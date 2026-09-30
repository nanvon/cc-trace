//! DSH 会话日志的 zstd 容器：帧边界扫描与单帧解码。
//!
//! DSH 的会话日志是「多帧拼接」的 zstd 容器：一个文件里顺序排列若干个互不依赖的
//! 完整帧。这个模块只做两件事：
//!
//! 1. [`scan`] 只解析帧头与块头，**不解压**，用来确定「哪些字节已经完整、可以入账」；
//! 2. [`decode_frame`] 解压**恰好一个完整帧**，多帧拼接与半截帧一律拒绝。
//!
//! 偏移语义：所有偏移都相对于传入缓冲区的起点，因此调用方必须让缓冲区**从某个帧的
//! 起点开始**（首次扫描传整个文件；增量扫描传「上一轮未完成帧的起点」到文件末尾）。
//! 规则逐条对应 DSH 随包 `@deepseek-ai/dsh-session-persistence-jsonl` 的
//! `scanZstdFrames()`，并与 cc-bar v1.1.1 的 `DshZstdFrames` 保持一致：
//! magic → frame header descriptor（保留位、FCS、single segment、checksum、字典）→
//! 逐块 block header → 可选 4 字节 checksum。**这里不校验 checksum**，
//! 它由 libzstd 在解码时校验（见 [`decode_frame`]）。
//!
//! 撕裂（文件正在被写入）不是失败：尾部未完成的帧留在 [`ScanResult::torn_start`]，
//! 等字节补齐后从该偏移重扫。损坏（magic 或保留位不对）则整份结果作废。

use std::io::Read;

/// zstd 帧 magic（小端字节序 `28 B5 2F FD`）。
pub const MAGIC: u32 = 0xFD2F_B528;

/// 单个完整帧的字节范围，`end` 为开区间。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRange {
    pub start: usize,
    pub end: usize,
}

/// 损坏位置与原因。损坏时调用方丢弃该文件本轮结果并保留旧水位：
/// 既不推进偏移也不入账，避免把坏文件的中段当成新数据。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corruption {
    InvalidMagic { byte_offset: usize },
    ReservedFrameHeaderBit { byte_offset: usize },
    ReservedBlockType { byte_offset: usize },
}

/// 一次扫描的结果。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScanResult {
    pub frames: Vec<FrameRange>,
    /// 有值表示尾部还有未完成帧；`None` 表示缓冲区正好结束在帧边界上。
    pub torn_start: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanOutcome {
    Scanned(ScanResult),
    Corrupt(Corruption),
}

/// 缓冲区里「已经完整、可以消费」的字节数：最后一个完整帧的末尾。
/// 撕裂尾帧与损坏都不会让它前进。
pub fn consumed_bytes(outcome: &ScanOutcome) -> usize {
    match outcome {
        ScanOutcome::Scanned(result) => result.frames.last().map_or(0, |frame| frame.end),
        ScanOutcome::Corrupt(_) => 0,
    }
}

/// 扫描缓冲区里所有完整帧。
///
/// `max_frames` 为 `Some(n)` 时只取前 n 个完整帧（读会话头元数据时用）。
pub fn scan(data: &[u8], max_frames: Option<usize>) -> ScanOutcome {
    let mut frames = Vec::new();
    let total = data.len();
    let mut offset = 0;

    while offset < total {
        let start = offset;

        if total - offset < 4 {
            return ScanOutcome::Scanned(ScanResult {
                frames,
                torn_start: Some(start),
            });
        }
        if u32::from_le_bytes(read4(data, offset)) != MAGIC {
            return ScanOutcome::Corrupt(Corruption::InvalidMagic {
                byte_offset: offset,
            });
        }
        offset += 4;

        // frame header descriptor；长度不够属于撕裂，不是损坏。
        if offset == total {
            return ScanOutcome::Scanned(ScanResult {
                frames,
                torn_start: Some(start),
            });
        }
        let descriptor = data[offset];
        offset += 1;

        // bit3／bit4 是保留位，必须为 0。
        if descriptor & 24 != 0 {
            return ScanOutcome::Corrupt(Corruption::ReservedFrameHeaderBit {
                byte_offset: offset - 1,
            });
        }

        let content_size_flag = usize::from(descriptor >> 6);
        let single_segment = descriptor & 32 != 0;
        let checksum = descriptor & 4 != 0;
        let dictionary_flag = usize::from(descriptor & 3);
        let dictionary_bytes = if dictionary_flag == 3 {
            4
        } else {
            dictionary_flag
        };
        let content_size_bytes = if content_size_flag == 0 {
            usize::from(single_segment)
        } else {
            1 << content_size_flag
        };
        // 非 single segment 时多一个 window descriptor 字节。
        let remaining_header_bytes =
            usize::from(!single_segment) + dictionary_bytes + content_size_bytes;

        if total - offset < remaining_header_bytes {
            return ScanOutcome::Scanned(ScanResult {
                frames,
                torn_start: Some(start),
            });
        }
        offset += remaining_header_bytes;

        // 逐块扫描，直到 last block。
        loop {
            if total - offset < 3 {
                return ScanOutcome::Scanned(ScanResult {
                    frames,
                    torn_start: Some(start),
                });
            }
            let block_header = read3(data, offset);
            offset += 3;

            let last_block = block_header & 1 != 0;
            let block_type = (block_header >> 1) & 3;
            let block_size = block_header >> 3;

            if block_type == 3 {
                return ScanOutcome::Corrupt(Corruption::ReservedBlockType {
                    byte_offset: offset - 3,
                });
            }
            // RLE 块的 payload 恒为 1 字节，其余按 blockSize。
            let payload_bytes = if block_type == 1 { 1 } else { block_size };
            if total - offset < payload_bytes {
                return ScanOutcome::Scanned(ScanResult {
                    frames,
                    torn_start: Some(start),
                });
            }
            offset += payload_bytes;

            if last_block {
                break;
            }
        }

        // 可选 4 字节 content checksum。
        if checksum {
            if total - offset < 4 {
                return ScanOutcome::Scanned(ScanResult {
                    frames,
                    torn_start: Some(start),
                });
            }
            offset += 4;
        }

        frames.push(FrameRange { start, end: offset });
        if max_frames.is_some_and(|limit| frames.len() == limit) {
            return ScanOutcome::Scanned(ScanResult {
                frames,
                torn_start: None,
            });
        }
    }

    ScanOutcome::Scanned(ScanResult {
        frames,
        torn_start: None,
    })
}

/// 单帧解压失败的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// 输入不是一个完整的 zstd 帧（长度与帧内声明不符，或根本不是帧）。
    InvalidFrame { expected: usize, actual: usize },
    /// 输入在帧结束前耗尽。
    TruncatedInput,
    /// libzstd 报告解码失败：损坏、checksum 不匹配等。`name` 是 libzstd 的错误名。
    CorruptedFrame { name: String },
    /// 解压输出超过上限（防损坏文件造成无界分配）。
    OutputTooLarge { limit: usize },
}

/// 单帧解压输出上限（256 MiB）。正常会话帧远小于此值，触顶即视为损坏。
pub const DEFAULT_OUTPUT_LIMIT: usize = 256 * 1024 * 1024;

/// 解码**一个完整帧**，返回解压后的字节。
///
/// checksum 由 libzstd 校验，这里不重复实现；多帧拼接与半截帧在此被拒。
pub fn decode_frame(frame: &[u8], output_limit: usize) -> Result<Vec<u8>, DecodeError> {
    if frame.len() < 4 {
        return Err(DecodeError::InvalidFrame {
            expected: 4,
            actual: frame.len(),
        });
    }

    // 先用扫描器确认输入**恰好是一个完整帧**。
    //
    // 不依赖解码器的「剩余输入」判断：`zstd::stream::read::Decoder` 内部包了
    // BufReader，会把源一次读空，多帧拼接在它眼里和单帧没有区别。
    match scan(frame, Some(1)) {
        ScanOutcome::Scanned(result)
            if result.frames.len() == 1 && result.frames[0].end == frame.len() => {}
        _ => {
            return Err(DecodeError::InvalidFrame {
                expected: frame.len(),
                actual: frame.len(),
            });
        }
    }

    let mut cursor: &[u8] = frame;
    let mut decoder = zstd::stream::read::Decoder::new(&mut cursor)
        .map_err(|error| DecodeError::CorruptedFrame {
            name: error.to_string(),
        })?
        .single_frame();

    let mut output = Vec::new();
    let mut chunk = vec![0_u8; 256 * 1024];
    loop {
        match decoder.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                if output.len() + read > output_limit {
                    return Err(DecodeError::OutputTooLarge {
                        limit: output_limit,
                    });
                }
                output.extend_from_slice(&chunk[..read]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(DecodeError::TruncatedInput);
            }
            Err(error) => {
                return Err(DecodeError::CorruptedFrame {
                    name: error.to_string(),
                });
            }
        }
    }

    Ok(output)
}

fn read4(data: &[u8], offset: usize) -> [u8; 4] {
    [
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]
}

fn read3(data: &[u8], offset: usize) -> usize {
    usize::from(data[offset])
        | usize::from(data[offset + 1]) << 8
        | usize::from(data[offset + 2]) << 16
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个完整帧。`level` 与内容一起决定压缩后帧的形态。
    fn frame(payload: &[u8], level: i32) -> Vec<u8> {
        zstd::encode_all(payload, level).expect("compress")
    }

    /// 造一个带 content checksum 的帧（libzstd 校验 checksum，扫描只跳 4 字节）。
    fn frame_with_checksum(payload: &[u8]) -> Vec<u8> {
        let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 3).expect("encoder");
        encoder.include_checksum(true).expect("checksum flag");
        std::io::Write::write_all(&mut encoder, payload).expect("write");
        encoder.finish().expect("finish")
    }

    #[test]
    fn a_single_frame_is_scanned_and_decoded() {
        let payload = b"{\"type\":\"session\",\"id\":\"s-1\"}\n";
        let data = frame(payload, 3);

        let ScanOutcome::Scanned(result) = scan(&data, None) else {
            panic!("expected a scanned outcome");
        };
        assert_eq!(result.frames.len(), 1);
        assert_eq!(result.frames[0].start, 0);
        assert_eq!(result.frames[0].end, data.len());
        assert_eq!(result.torn_start, None);

        let decoded = decode_frame(&data, DEFAULT_OUTPUT_LIMIT).expect("decode");
        assert_eq!(decoded, payload);
    }

    #[test]
    fn concatenated_frames_are_scanned_individually() {
        let mut data = frame(b"first\n", 3);
        let second_start = data.len();
        data.extend(frame(b"second\n", 3));

        let ScanOutcome::Scanned(result) = scan(&data, None) else {
            panic!("expected a scanned outcome");
        };
        assert_eq!(result.frames.len(), 2);
        assert_eq!(result.frames[1].start, second_start);
        assert_eq!(
            consumed_bytes(&ScanOutcome::Scanned(result.clone())),
            data.len()
        );

        let first = &data[result.frames[0].start..result.frames[0].end];
        let second = &data[result.frames[1].start..result.frames[1].end];
        assert_eq!(
            decode_frame(first, DEFAULT_OUTPUT_LIMIT).expect("first"),
            b"first\n"
        );
        assert_eq!(
            decode_frame(second, DEFAULT_OUTPUT_LIMIT).expect("second"),
            b"second\n"
        );
    }

    #[test]
    fn a_torn_tail_keeps_the_remaining_bytes_out_of_accounting() {
        let mut data = frame(b"complete\n", 3);
        let torn_start = data.len();
        data.extend(&frame(b"incomplete payload that is long enough to span blocks", 3)[..10]);

        let ScanOutcome::Scanned(result) = scan(&data, None) else {
            panic!("expected a scanned outcome");
        };
        assert_eq!(result.frames.len(), 1);
        assert_eq!(result.torn_start, Some(torn_start));
        assert_eq!(
            consumed_bytes(&ScanOutcome::Scanned(result)),
            torn_start,
            "撕裂尾帧不能推进水位"
        );
    }

    #[test]
    fn an_invalid_magic_discards_the_whole_file() {
        let data = b"not a zstd frame at all";
        assert_eq!(
            scan(data, None),
            ScanOutcome::Corrupt(Corruption::InvalidMagic { byte_offset: 0 })
        );
        assert_eq!(consumed_bytes(&scan(data, None)), 0);
    }

    #[test]
    fn a_reserved_frame_header_bit_is_corruption() {
        let mut data = frame(b"payload\n", 3);
        // descriptor 在 magic 之后一跳；置上保留位 bit3。
        data[4] |= 8;
        assert_eq!(
            scan(&data, None),
            ScanOutcome::Corrupt(Corruption::ReservedFrameHeaderBit { byte_offset: 4 })
        );
    }

    #[test]
    fn a_frame_with_checksum_scans_past_the_checksum() {
        let payload = b"{\"type\":\"session\",\"id\":\"s-2\"}\n";
        let data = frame_with_checksum(payload);

        let ScanOutcome::Scanned(result) = scan(&data, None) else {
            panic!("expected a scanned outcome");
        };
        assert_eq!(result.frames.len(), 1);
        assert_eq!(result.frames[0].end, data.len());
        assert_eq!(
            decode_frame(&data, DEFAULT_OUTPUT_LIMIT).expect("decode"),
            payload
        );
    }

    #[test]
    fn max_frames_stops_early() {
        let mut data = frame(b"a\n", 3);
        data.extend(frame(b"b\n", 3));

        let ScanOutcome::Scanned(result) = scan(&data, Some(1)) else {
            panic!("expected a scanned outcome");
        };
        assert_eq!(result.frames.len(), 1);
        assert_eq!(result.torn_start, None);
    }

    #[test]
    fn decoding_rejects_two_frames_at_once() {
        let mut data = frame(b"a\n", 3);
        data.extend(frame(b"b\n", 3));

        assert!(matches!(
            decode_frame(&data, DEFAULT_OUTPUT_LIMIT),
            Err(DecodeError::InvalidFrame { .. })
        ));
    }

    #[test]
    fn decoding_rejects_trailing_garbage_after_a_frame() {
        let mut data = frame(b"a\n", 3);
        data.extend(b"trailing");

        assert!(matches!(
            decode_frame(&data, DEFAULT_OUTPUT_LIMIT),
            Err(DecodeError::InvalidFrame { .. })
        ));
    }

    #[test]
    fn decoding_rejects_a_truncated_frame() {
        let data = frame(b"a fairly long payload to make blocks non-empty\n", 3);
        let truncated = &data[..data.len() - 4];

        // 结构校验由扫描器负责：长度与帧内声明不符在这里就被拒，
        // 不会把半截帧交给 libzstd。`TruncatedInput` 只留给扫描器放行、
        // 解码阶段仍然提前耗尽输入的极端情况。
        assert!(matches!(
            decode_frame(truncated, DEFAULT_OUTPUT_LIMIT),
            Err(DecodeError::InvalidFrame { .. })
        ));
    }

    #[test]
    fn a_corrupted_frame_is_reported_by_libzstd() {
        let payload = vec![b'x'; 4096];
        let mut data = frame(&payload, 1);
        // 破坏最后一个块的内容，但保持帧长度不变。
        let last = data.len() - 1;
        data[last] ^= 0xFF;

        let outcome = decode_frame(&data, DEFAULT_OUTPUT_LIMIT);
        assert!(
            matches!(outcome, Err(DecodeError::CorruptedFrame { .. })),
            "expected a corruption error, got {outcome:?}"
        );
    }

    #[test]
    fn decoding_respects_the_output_limit() {
        let payload = vec![b'y'; 4096];
        let data = frame(&payload, 1);

        assert!(matches!(
            decode_frame(&data, 64),
            Err(DecodeError::OutputTooLarge { limit: 64 })
        ));
    }

    #[test]
    fn a_short_input_is_not_a_frame() {
        assert!(matches!(
            decode_frame(b"abc", DEFAULT_OUTPUT_LIMIT),
            Err(DecodeError::InvalidFrame { .. })
        ));
    }
}

#!/bin/sh
# cargo runner：运行前用固定身份重新签名，然后把控制权交给二进制本身。
#
# 为什么需要：cargo 产出的是 adhoc 签名（`Signature=adhoc`），没有稳定的签名标识，
# 钥匙串 ACL 只能按 CDHash 记录被授权的程序，而 CDHash 随二进制内容变化。于是每次
# 重新编译后读取 Claude Code 凭据都要重新授权一次。用固定证书签名后，ACL 记录的是
# designated requirement（identifier + 证书），重新编译不影响匹配。
# 背景见 docs/决策/ADR-0013-macOS读取ClaudeCode钥匙串凭据.md。
#
# 身份来源：`CC_TRACE_SIGN_IDENTITY` 优先，否则取第一个有效的代码签名身份。
# 一个都没有时跳过签名并继续运行——签名是开发便利，不是运行前提，不能卡住 dev。

set -e

BIN="$1"
if [ -z "$BIN" ]; then
	echo "dev-sign: 缺少要运行的二进制路径" >&2
	exit 64
fi
shift

# 只签正式产物。测试二进制在 target/*/deps/ 下，名字带 hash 且每次都不同，
# 签它没有意义（ACL 本来就匹配不上），只会给 `cargo test` 增加无谓开销。
case "$BIN" in
*/deps/*) exec "$BIN" "$@" ;;
esac

if [ "$(uname -s)" != "Darwin" ]; then
	exec "$BIN" "$@"
fi

identity="$CC_TRACE_SIGN_IDENTITY"
if [ -z "$identity" ]; then
	identity=$(security find-identity -v -p codesigning 2>/dev/null |
		sed -n 's/^ *1) [0-9A-F]* "\(.*\)"$/\1/p')
fi

if [ -z "$identity" ]; then
	echo "dev-sign: 没有可用的代码签名身份，跳过签名。" >&2
	echo "dev-sign: 钥匙串授权会在每次重新编译后重新索要，见 ADR-0013。" >&2
	exec "$BIN" "$@"
fi

# 签名标识必须显式指定。cargo 默认写的是 `cc_trace-<metadata hash>`，那个 hash 由构建
# 配置决定，改 feature、profile 或依赖都可能让它变，而它是 designated requirement 的一部分。
# 钉死成 tauri.conf.json 里的 identifier，dev 与打包产物的 DR 才是同一个。
root=$(cd "$(dirname "$0")/.." && pwd)
bundle_id=$(sed -n 's/.*"identifier": *"\([^"]*\)".*/\1/p' "$root/src-tauri/tauri.conf.json" | head -1)
if [ -z "$bundle_id" ]; then
	echo "dev-sign: 从 tauri.conf.json 读不到 identifier，跳过签名。" >&2
	exec "$BIN" "$@"
fi

# --force 覆盖 cargo 留下的 adhoc 签名。
# 不加 --options runtime：hardened runtime 会限制 WebView 的 JIT，开发期不需要。
if codesign --force --sign "$identity" --identifier "$bundle_id" \
	--timestamp=none "$BIN" 2>/tmp/cc-trace-codesign.log; then
	echo "dev-sign: 已签名 $bundle_id（$identity）" >&2
else
	echo "dev-sign: 签名失败，仍然继续运行。原因：" >&2
	cat /tmp/cc-trace-codesign.log >&2
fi

exec "$BIN" "$@"

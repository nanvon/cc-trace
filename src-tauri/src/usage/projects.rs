//! 项目身份：把一个工作目录归一成稳定的项目键。
//!
//! 规则见 [ADR-0034]，全部要点只有两条：**能查才查、查不准就不猜**。
//!
//! - 家目录以外、以及受保护目录（桌面、文稿、下载、音乐、图片、影片）下的路径
//!   不做任何文件系统调用，只按字符串归组，状态标 `unverified`：
//!   非沙盒应用去访问这些目录会触发系统的「文件与文件夹」授权弹窗。
//! - 允许检查的路径才解析符号链接、向上找最近的 Git 根；
//!   worktree（`.git` 是文件）归入主仓库身份，明细里单独保留 worktree 路径。
//! - 解析不到 Git 根时用规范化后的原路径当项目键，不是「没有项目」。
//! - 根目录、家目录、空 cwd 归到**未归属**：它们不是项目。
//! - CC Trace 自己的探针目录单列为**系统项目**，不混进用户项目。
//!
//! [ADR-0034]: ../../../../docs/决策/ADR-0034-项目身份与未归属口径.md

use std::path::{Component, Path, PathBuf};

use crate::contracts::UsageProjectSummary;

/// 项目身份的可信度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectStatus {
    /// 路径存在，做过文件系统检查。
    Available,
    /// 路径不存在（日志里留下的历史 cwd）。
    Unavailable,
    /// 不允许检查，只按字符串归组。
    Unverified,
    /// CC Trace 自己的探针目录。
    System,
    /// 根目录、家目录或空 cwd：不属于任何项目。
    Unassigned,
}

impl ProjectStatus {
    pub fn key(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::Unverified => "unverified",
            Self::System => "system",
            Self::Unassigned => "unassigned",
        }
    }

    /// 界面是否应当声称这个项目「存在」。
    pub fn claims_existence(self) -> bool {
        matches!(self, Self::Available | Self::Unavailable)
    }
}

/// 一次解析的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectResolution {
    /// 项目身份键：Git 根（能解析到时）或规范化后的工作目录。
    pub project_key: Option<String>,
    /// 对话自身的工作目录（worktree 明细用）。
    pub worktree_path: Option<String>,
    /// Git 根；不是 Git 仓库时为 `None`。
    pub repo_root: Option<String>,
    /// 展示名（项目键的路径尾段）。
    pub hint: Option<String>,
    pub status: ProjectStatus,
}

impl ProjectResolution {
    fn unassigned() -> Self {
        Self {
            project_key: None,
            worktree_path: None,
            repo_root: None,
            hint: None,
            status: ProjectStatus::Unassigned,
        }
    }
}

/// 受保护目录名（大小写不敏感）。macOS 的 TCC 目录与 Windows 的对应目录同名；
/// OneDrive 重定向后的 `~/OneDrive/Documents/...` 也命中，因为判定看的是任一层面。
const PROTECTED_COMPONENTS: [&str; 6] = [
    "desktop",
    "documents",
    "downloads",
    "music",
    "pictures",
    "movies",
];

/// 系统探针目录的标记：日志里出现这些名字说明是自动化探针留下的，不是用户项目。
const SYSTEM_MARKERS: [&str; 5] = [
    "cc-trace-probe",
    "cctrace-probe",
    "ccbar-codex-wakeup",
    "claudeprobe",
    "cctrace-system",
];

/// 向上找 Git 根的最大层数。超过即放弃：网络宗卷上的深目录不该把刷新拖住。
const MAX_GIT_WALK: usize = 32;

/// 解析一个工作目录。`home` 为 `None` 时一律按「不允许检查」处理。
pub fn resolve(cwd: &str, home: Option<&Path>) -> ProjectResolution {
    let trimmed = cwd.trim();
    if trimmed.is_empty() {
        return ProjectResolution::unassigned();
    }

    let raw = PathBuf::from(trimmed);
    if is_root_like(&raw, home) {
        return ProjectResolution::unassigned();
    }
    if has_system_marker(&raw) {
        return ProjectResolution {
            worktree_path: Some(normalized_text(&raw)),
            status: ProjectStatus::System,
            ..ProjectResolution::unassigned()
        };
    }

    let normalized = normalized_text(&raw);
    let checkable = home.is_some_and(|home| allows_file_system_check(&raw, home));

    if !checkable {
        // 只按字符串归组：不解析符号链接、不看 `.git`、不判断存在性。
        return ProjectResolution {
            hint: tail_segment(&normalized),
            project_key: Some(normalized.clone()),
            worktree_path: Some(normalized),
            repo_root: None,
            status: ProjectStatus::Unverified,
        };
    }

    let exists = raw.exists();
    let resolved = std::fs::canonicalize(&raw).unwrap_or_else(|_| raw.clone());
    let resolved_text = normalized_text(&resolved);

    match find_git_root(&resolved) {
        Some(root) => {
            let root_text = normalized_text(&root);
            ProjectResolution {
                hint: tail_segment(&root_text),
                project_key: Some(root_text.clone()),
                worktree_path: Some(resolved_text),
                repo_root: Some(root_text),
                status: ProjectStatus::Available,
            }
        }
        None => ProjectResolution {
            hint: tail_segment(&resolved_text),
            project_key: Some(resolved_text.clone()),
            worktree_path: Some(resolved_text),
            repo_root: None,
            status: if exists {
                ProjectStatus::Available
            } else {
                ProjectStatus::Unavailable
            },
        },
    }
}

/// 「无明确项目」的保留键：空 cwd、家目录、根目录。与未归属（key 空串）分开存放。
pub const KEY_NONE: &str = "@none";
/// 「系统任务」的保留键：CC Trace 探针目录。
pub const KEY_SYSTEM: &str = "@system";

/// 保留键：界面按键显示固定名称，不当作路径。
pub fn is_reserved_key(key: &str) -> bool {
    key == KEY_NONE || key == KEY_SYSTEM
}

/// 扫描层用的入口：解析结果带进程内缓存，避免每一行日志都做文件系统调用。
///
/// 缓存有界（满了整表清空重来）。目录状态会随时间变化（仓库被删除、新建 `.git`），
/// 但一次进程生命周期内沿用同一结果是可接受的：「重新计算用量」会重启进程内缓存以外的全部状态，
/// 且身份只影响分组，不影响费用与 Token。
pub fn resolve_cached(cwd: &str) -> ProjectResolution {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    const CACHE_LIMIT: usize = 4096;
    static CACHE: OnceLock<Mutex<HashMap<String, ProjectResolution>>> = OnceLock::new();

    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock()
        && let Some(hit) = guard.get(cwd)
    {
        return hit.clone();
    }
    let home = crate::providers::credentials::home_dir();
    let resolution = resolve(cwd, home.as_deref());
    if let Ok(mut guard) = cache.lock() {
        if guard.len() >= CACHE_LIMIT {
            guard.clear();
        }
        guard.insert(cwd.to_owned(), resolution.clone());
    }
    resolution
}

/// 给项目列表的一行补上目录状态。只对允许检查的路径访问文件系统，见 [`resolve`]。
pub fn annotate_summary(item: &mut UsageProjectSummary, home: Option<&Path>) {
    if item.unattributed || item.key.is_empty() {
        item.status = "unattributed".to_owned();
        return;
    }
    if is_reserved_key(&item.key) {
        item.status = "reserved".to_owned();
        return;
    }
    let resolution = resolve(&item.key, home);
    item.status = resolution.status.key().to_owned();
    if resolution.status.claims_existence() {
        item.is_git = Some(resolution.repo_root.is_some());
    }
}

/// 根目录、家目录本身、以及盘符根：它们不是项目。
fn is_root_like(path: &Path, home: Option<&Path>) -> bool {
    if path.parent().is_none() {
        return true;
    }
    // Windows 的 `C:\` 在 `Path` 里父级为空，同样命中上面一条。
    if let Some(home) = home
        && same_path(path, home)
    {
        return true;
    }
    false
}

/// 路径比较：能取到规范路径就比规范路径，取不到就比字符串（大小写不敏感）。
fn same_path(left: &Path, right: &Path) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left.to_string_lossy().to_lowercase() == right.to_string_lossy().to_lowercase()
}

fn has_system_marker(path: &Path) -> bool {
    path.components().any(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        let name = name.to_string_lossy().to_lowercase();
        SYSTEM_MARKERS.iter().any(|marker| name.contains(marker))
    })
}

/// 是否允许对这个路径做文件系统检查。
///
/// 条件只有两个：在家目录之内，且没有哪一层是受保护目录。
/// 拿不准时不检查——宁可少查一次，也不弹一次系统授权框。
fn allows_file_system_check(path: &Path, home: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(home) else {
        // 家目录以外：可能是可移动卷或网络宗卷，检查会触发授权或长时间等待。
        return false;
    };
    !relative.components().any(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        let name = name.to_string_lossy().to_lowercase();
        PROTECTED_COMPONENTS
            .iter()
            .any(|protected| name == *protected)
    })
}

/// 向上找最近的 Git 根。`.git` 是目录时它就是根；是文件时说明这里是 worktree，
/// 顺着 `gitdir` 与 `commondir` 找到主仓库。
fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut cursor = Some(start);
    let mut depth = 0;
    while let Some(path) = cursor {
        if depth > MAX_GIT_WALK {
            return None;
        }
        depth += 1;
        let marker = path.join(".git");
        if marker.is_dir() {
            return Some(path.to_path_buf());
        }
        if marker.is_file() {
            // worktree：`.git` 文件里写着 `gitdir: <主仓库>/.git/worktrees/<名字>`。
            return main_repo_root(&marker).or_else(|| Some(path.to_path_buf()));
        }
        cursor = path.parent();
    }
    None
}

/// 从 worktree 的 `.git` 文件推出主仓库根。
fn main_repo_root(git_file: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(git_file).ok()?;
    let gitdir = content
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:"))
        .map(str::trim)?;
    let gitdir = PathBuf::from(gitdir);
    let gitdir = if gitdir.is_absolute() {
        gitdir
    } else {
        git_file.parent()?.join(gitdir)
    };

    // 优先看 `commondir`：它指回主仓库的 `.git`（worktree 的 gitdir 在
    // `<主仓库>/.git/worktrees/<名字>`）。
    //
    // 拼接后必须做**词法归一**：`gitdir.join("../..")` 里保留着字面量 `..`，
    // 直接取 `parent()` 会得到 `<主仓库>/.git/worktrees`（按字符串走层），
    // 而不是主仓库根。这里不调用 `canonicalize`：主仓库可能在只读或网络位置上，
    // 解析路径不该依赖它「当时能被打开」。
    if let Ok(common) = std::fs::read_to_string(gitdir.join("commondir")) {
        let common = common.trim();
        let common = if Path::new(common).is_absolute() {
            PathBuf::from(common)
        } else {
            gitdir.join(common)
        };
        if let Some(root) = normalize_lexically(&common).parent() {
            return Some(root.to_path_buf());
        }
    }

    // 没有 commondir 时按 `.git/worktrees/<名字>` 的形状回推：
    // `<名字>` → `worktrees` → `.git` → 主仓库根。
    gitdir
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

/// 词法归一：消掉 `.` 与 `..`，不访问文件系统、不解析符号链接。
///
/// 越出根目录的 `..` 直接丢弃：那条路径本来就没有意义，比留一个相对前缀更安全。
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !output.pop() {
                    continue;
                }
            }
            other => output.push(other.as_os_str()),
        }
    }
    output
}

/// 去掉尾部分隔符并限长。保留原分隔符：Windows 路径仍以反斜杠展示。
fn normalized_text(path: &Path) -> String {
    let mut text = path.to_string_lossy().to_string();
    while text.len() > 1 && (text.ends_with('/') || text.ends_with('\\')) {
        text.pop();
    }
    text.chars().take(1024).collect()
}

/// 路径尾段，用作展示名。不调用文件系统。
fn tail_segment(path: &str) -> Option<String> {
    path.rsplit(['/', '\\'])
        .find(|segment| !segment.is_empty())
        .map(|segment| segment.chars().take(120).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> tempfile::TempDir {
        tempfile::tempdir().expect("home")
    }

    /// 造一个 Git 仓库（只建 `.git` 目录，不需要真的 git）。
    fn git_repo(root: &Path) {
        std::fs::create_dir_all(root.join(".git")).expect("git dir");
    }

    /// 造一个 worktree：`.git` 是文件，gitdir 指向主仓库的 `.git/worktrees/<名字>`。
    fn worktree(main: &Path, worktree_path: &Path, name: &str) {
        git_repo(main);
        let gitdir = main.join(".git").join("worktrees").join(name);
        std::fs::create_dir_all(&gitdir).expect("gitdir");
        std::fs::write(gitdir.join("commondir"), "../..\n").expect("commondir");
        std::fs::create_dir_all(worktree_path).expect("worktree dir");
        std::fs::write(
            worktree_path.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .expect(".git file");
    }

    #[test]
    fn an_empty_or_root_like_path_is_unassigned() {
        let home = home();
        assert_eq!(
            resolve("", Some(home.path())),
            ProjectResolution::unassigned()
        );
        assert_eq!(
            resolve("  ", Some(home.path())),
            ProjectResolution::unassigned()
        );
        assert_eq!(
            resolve("/", Some(home.path())),
            ProjectResolution::unassigned()
        );
        let home_text = home.path().to_string_lossy().to_string();
        assert_eq!(
            resolve(&home_text, Some(home.path())),
            ProjectResolution::unassigned()
        );
    }

    #[test]
    fn a_git_repo_resolves_to_its_root_with_a_display_name() {
        let home = home();
        let repo = home.path().join("code").join("myrepo").join("src");
        std::fs::create_dir_all(&repo).expect("dirs");
        git_repo(&home.path().join("code").join("myrepo"));

        let resolved = resolve(&repo.to_string_lossy(), Some(home.path()));
        assert_eq!(resolved.status, ProjectStatus::Available);
        assert_eq!(resolved.hint.as_deref(), Some("myrepo"));
        assert!(
            resolved
                .project_key
                .as_deref()
                .is_some_and(|key| key.ends_with("myrepo"))
        );
        // worktree 路径保留对话自己的工作目录。
        assert!(
            resolved
                .worktree_path
                .as_deref()
                .is_some_and(|path| path.ends_with("src"))
        );
    }

    #[test]
    fn a_worktree_resolves_to_the_main_repository() {
        let home = home();
        let main = home.path().join("code").join("main-repo");
        let worktree_path = home.path().join("code").join("feature-branch");
        worktree(&main, &worktree_path, "feature-branch");

        let resolved = resolve(&worktree_path.to_string_lossy(), Some(home.path()));
        assert_eq!(resolved.status, ProjectStatus::Available);
        assert_eq!(
            resolved.hint.as_deref(),
            Some("main-repo"),
            "展示名取主仓库"
        );
        assert!(
            resolved
                .repo_root
                .as_deref()
                .is_some_and(|root| root.ends_with("main-repo")),
            "{:?}",
            resolved.repo_root
        );
        // worktree 自己的路径仍要留在明细里。
        assert!(
            resolved
                .worktree_path
                .as_deref()
                .is_some_and(|path| path.ends_with("feature-branch"))
        );
    }

    #[test]
    fn a_directory_without_git_is_its_own_project() {
        let home = home();
        let plain = home.path().join("notes");
        std::fs::create_dir_all(&plain).expect("dirs");

        let resolved = resolve(&plain.to_string_lossy(), Some(home.path()));
        assert_eq!(resolved.status, ProjectStatus::Available);
        assert_eq!(resolved.hint.as_deref(), Some("notes"));
        assert_eq!(resolved.repo_root, None);
    }

    #[test]
    fn a_missing_directory_is_marked_unavailable() {
        let home = home();
        let gone = home.path().join("deleted").join("project");

        let resolved = resolve(&gone.to_string_lossy(), Some(home.path()));
        assert_eq!(resolved.status, ProjectStatus::Unavailable);
        assert!(resolved.project_key.is_some(), "历史路径仍要能归组");
    }

    #[test]
    fn protected_directories_are_not_touched() {
        let home = home();
        let desktop = home.path().join("Desktop").join("scratch");
        std::fs::create_dir_all(&desktop).expect("dirs");
        // 就算这里真有 Git 仓库，也不去查。
        git_repo(&home.path().join("Desktop").join("scratch"));

        let resolved = resolve(&desktop.to_string_lossy(), Some(home.path()));
        assert_eq!(resolved.status, ProjectStatus::Unverified);
        assert_eq!(resolved.repo_root, None);
        assert!(
            resolved
                .project_key
                .as_deref()
                .is_some_and(|key| key.ends_with("scratch"))
        );
    }

    #[test]
    fn onedrive_redirected_protected_directories_are_also_skipped() {
        let home = home();
        let redirected = home.path().join("OneDrive").join("Documents").join("proj");

        let resolved = resolve(&redirected.to_string_lossy(), Some(home.path()));
        assert_eq!(
            resolved.status,
            ProjectStatus::Unverified,
            "重定向后的受保护目录同样不查"
        );
    }

    #[test]
    fn paths_outside_the_home_directory_are_not_touched() {
        let home = home();
        let outside = tempfile::tempdir().expect("outside");
        git_repo(outside.path());

        let resolved = resolve(&outside.path().to_string_lossy(), Some(home.path()));
        assert_eq!(resolved.status, ProjectStatus::Unverified);
        assert_eq!(resolved.repo_root, None);
    }

    #[test]
    fn without_a_home_directory_nothing_is_checked() {
        let dir = tempfile::tempdir().expect("dir");
        git_repo(dir.path());

        let resolved = resolve(&dir.path().to_string_lossy(), None);
        assert_eq!(resolved.status, ProjectStatus::Unverified);
    }

    #[test]
    fn probe_directories_are_marked_as_system() {
        let home = home();
        let probe = home.path().join(".cc-trace-probe").join("2026-10-01");

        let resolved = resolve(&probe.to_string_lossy(), Some(home.path()));
        assert_eq!(resolved.status, ProjectStatus::System);
        assert_eq!(resolved.project_key, None, "系统项目不参与项目聚合");
    }

    #[test]
    fn lexical_normalization_removes_dot_segments() {
        assert_eq!(
            normalize_lexically(Path::new("/a/b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(
            normalize_lexically(Path::new("/a/./b")),
            PathBuf::from("/a/b")
        );
        // 越出根的 `..` 被丢弃，而不是留下相对前缀。
        assert_eq!(normalize_lexically(Path::new("/../a")), PathBuf::from("/a"));
    }

    #[test]
    fn trailing_separators_do_not_change_the_identity() {
        let home = home();
        let plain = home.path().join("notes");
        std::fs::create_dir_all(&plain).expect("dirs");

        let with = resolve(&format!("{}/", plain.to_string_lossy()), Some(home.path()));
        let without = resolve(&plain.to_string_lossy(), Some(home.path()));
        assert_eq!(with.project_key, without.project_key);
    }
}

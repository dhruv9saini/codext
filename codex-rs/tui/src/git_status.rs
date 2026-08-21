use std::path::Path;

use tokio::process::Command;
use tokio::time::Duration;
use tokio::time::timeout;

const GIT_STATUS_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GitStatusSummary {
    pub(crate) branch: String,
    pub(crate) changed: usize,
    pub(crate) untracked: usize,
    pub(crate) ahead: usize,
    pub(crate) behind: usize,
}

pub(crate) async fn collect_git_status_summary(cwd: &Path) -> Option<GitStatusSummary> {
    let output = run_git_command(&["rev-parse", "--is-inside-work-tree"], cwd).await?;
    if !output.status.success() {
        return None;
    }
    let output = run_git_command(
        &["status", "--porcelain=2", "-z", "--untracked-files=normal"],
        cwd,
    )
    .await?;
    if !output.status.success() {
        return None;
    }
    let (changed, untracked) = parse_porcelain_counts(&output.stdout);
    let branch = string_output(run_git_command(&["branch", "--show-current"], cwd).await)
        .filter(|branch| !branch.is_empty());
    let branch = match branch {
        Some(branch) => branch,
        None => string_output(run_git_command(&["rev-parse", "--short", "HEAD"], cwd).await)
            .map(|sha| format!("detached@{sha}"))
            .unwrap_or_else(|| "detached".to_string()),
    };
    let ahead = commit_count(cwd, "@{u}..HEAD").await.unwrap_or(0);
    let behind = commit_count(cwd, "HEAD..@{u}").await.unwrap_or(0);
    Some(GitStatusSummary {
        branch,
        changed,
        untracked,
        ahead,
        behind,
    })
}

fn parse_porcelain_counts(output: &[u8]) -> (usize, usize) {
    let mut changed = 0;
    let mut untracked = 0;
    for entry in output
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        match entry[0] {
            b'?' => untracked += 1,
            b'1' | b'2' | b'u' => changed += 1,
            _ => {}
        }
    }
    (changed, untracked)
}

fn string_output(output: Option<std::process::Output>) -> Option<String> {
    let output = output?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_string())
}

async fn commit_count(cwd: &Path, range: &str) -> Option<usize> {
    string_output(run_git_command(&["rev-list", "--count", range], cwd).await)?
        .parse()
        .ok()
}

async fn run_git_command(args: &[&str], cwd: &Path) -> Option<std::process::Output> {
    let mut command = Command::new("git");
    command
        .args(["-c", "core.fsmonitor=false"])
        .args(args)
        .current_dir(cwd)
        .kill_on_drop(true);
    timeout(GIT_STATUS_TIMEOUT, command.output())
        .await
        .ok()?
        .ok()
}

#[cfg(test)]
mod tests {
    use super::parse_porcelain_counts;
    use pretty_assertions::assert_eq;

    #[test]
    fn porcelain_counts_track_changed_and_untracked_entries() {
        let output = b"1 M. N... 100644 100644 100644 a b tracked\0\
2 R. N... 100644 100644 100644 a b R100 renamed\0old\0\
u UU N... 100644 100644 100644 100644 a b c conflict\0\
? new-file\0! ignored\0";

        assert_eq!(parse_porcelain_counts(output), (3, 1));
    }
}

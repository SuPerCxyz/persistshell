use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use persist_core::shell_state::{
    EnvironmentPolicy, ShellStateIdentity, STATE_ENV_INCLUDE, STATE_ENV_MAX_BYTES,
    STATE_ENV_MAX_VARIABLES, STATE_ENV_POLICY_FINGERPRINT,
};
use persist_core::{PersistError, RecoveryEnvironmentConfig, Result};

pub struct ShellLaunch {
    pub environment: Vec<(String, String)>,
    pub arguments: Vec<String>,
}

#[cfg(test)]
pub fn prepare(
    shell: &str,
    session_id: u32,
    history_dir: &Path,
    helper: &Path,
    identity: &ShellStateIdentity,
) -> Result<Option<ShellLaunch>> {
    prepare_with_policy(
        shell,
        session_id,
        history_dir,
        helper,
        identity,
        &RecoveryEnvironmentConfig::default(),
    )
}

pub fn prepare_with_policy(
    shell: &str,
    session_id: u32,
    history_dir: &Path,
    helper: &Path,
    identity: &ShellStateIdentity,
    environment_config: &RecoveryEnvironmentConfig,
) -> Result<Option<ShellLaunch>> {
    if !helper.is_file() {
        return Ok(None);
    }
    let policy_hint = PolicyHint::new(environment_config)?;
    match shell_name(shell) {
        Some("bash") => {
            prepare_bash(session_id, history_dir, helper, identity, &policy_hint).map(Some)
        }
        Some("zsh") => {
            prepare_zsh(session_id, history_dir, helper, identity, &policy_hint).map(Some)
        }
        Some("fish") => {
            prepare_fish(session_id, history_dir, helper, identity, &policy_hint).map(Some)
        }
        _ => Ok(None),
    }
}

struct PolicyHint {
    include: String,
    max_variables: String,
    max_bytes: String,
    fingerprint: String,
}

impl PolicyHint {
    fn new(config: &RecoveryEnvironmentConfig) -> Result<Self> {
        let max_bytes = usize::try_from(config.max_bytes.bytes())
            .map_err(|_| PersistError::invalid_argument("environment byte limit is invalid"))?;
        let policy = EnvironmentPolicy::new(&config.include, config.max_variables, max_bytes)?;
        Ok(Self {
            include: config.include.join(","),
            max_variables: config.max_variables.to_string(),
            max_bytes: max_bytes.to_string(),
            fingerprint: policy.fingerprint().to_owned(),
        })
    }
}

pub fn helper_path() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("PERSIST_TEST_HELPER_PATH").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let executable = std::env::current_exe().ok()?;
    let parent = executable.parent()?;
    let direct = parent.join("persist");
    if direct.is_file() {
        return Some(direct);
    }
    parent.parent().map(|path| path.join("persist"))
}

fn prepare_bash(
    session_id: u32,
    history_dir: &Path,
    helper: &Path,
    identity: &ShellStateIdentity,
    policy_hint: &PolicyHint,
) -> Result<ShellLaunch> {
    let hook_dir = private_hook_dir(history_dir, session_id)?;
    write_private(&hook_dir.join("status"), "enabled\n")?;
    let rcfile = hook_dir.join("bashrc");
    write_private(&rcfile, BASH_RC)?;
    let mut environment = common_environment(session_id, helper, identity, policy_hint);
    environment.push((
        "PERSIST_STATE_STATUS".into(),
        hook_dir.join("state-status").to_string_lossy().into_owned(),
    ));
    Ok(ShellLaunch {
        environment,
        arguments: vec![
            "--rcfile".into(),
            rcfile.to_string_lossy().into_owned(),
            "-i".into(),
        ],
    })
}

fn prepare_zsh(
    session_id: u32,
    history_dir: &Path,
    helper: &Path,
    identity: &ShellStateIdentity,
    policy_hint: &PolicyHint,
) -> Result<ShellLaunch> {
    let hook_dir = private_hook_dir(history_dir, session_id)?;
    write_private(&hook_dir.join("status"), "enabled\n")?;
    write_private(&hook_dir.join(".zshenv"), ZSH_ENV)?;
    write_private(&hook_dir.join(".zprofile"), ZSH_PROFILE)?;
    write_private(&hook_dir.join(".zshrc"), ZSH_RC)?;
    let mut environment = common_environment(session_id, helper, identity, policy_hint);
    let original = std::env::var("ZDOTDIR").ok();
    environment.push((
        "PERSIST_ORIGINAL_ZDOTDIR".into(),
        original.clone().unwrap_or_default(),
    ));
    environment.push((
        "PERSIST_ORIGINAL_ZDOTDIR_SET".into(),
        if original.is_some() { "1" } else { "0" }.into(),
    ));
    environment.push((
        "PERSIST_HOOK_DIR".into(),
        hook_dir.to_string_lossy().into_owned(),
    ));
    environment.push((
        "PERSIST_HISTORY_STATUS".into(),
        hook_dir.join("status").to_string_lossy().into_owned(),
    ));
    environment.push(("ZDOTDIR".into(), hook_dir.to_string_lossy().into_owned()));
    Ok(ShellLaunch {
        environment,
        arguments: vec!["-i".into()],
    })
}

fn prepare_fish(
    session_id: u32,
    history_dir: &Path,
    helper: &Path,
    identity: &ShellStateIdentity,
    policy_hint: &PolicyHint,
) -> Result<ShellLaunch> {
    let hook_dir = private_hook_dir(history_dir, session_id)?;
    write_private(&hook_dir.join("status"), "enabled\n")?;
    let mut environment = common_environment(session_id, helper, identity, policy_hint);
    environment.push((
        "PERSIST_HISTORY_STATUS".into(),
        hook_dir.join("status").to_string_lossy().into_owned(),
    ));
    Ok(ShellLaunch {
        environment,
        arguments: vec!["--init-command".into(), FISH_INIT.into(), "-i".into()],
    })
}

fn common_environment(
    session_id: u32,
    helper: &Path,
    identity: &ShellStateIdentity,
    policy_hint: &PolicyHint,
) -> Vec<(String, String)> {
    vec![
        (
            "PERSIST_HISTORY_HELPER".into(),
            helper.to_string_lossy().into_owned(),
        ),
        ("PERSIST_SESSION_ID".into(), session_id.to_string()),
        ("PERSIST_STATE_FILE".into(), identity.path_string()),
        (
            "PERSIST_STATE_SESSION_ID".into(),
            identity.session_id().to_string(),
        ),
        (
            "PERSIST_STATE_INCARNATION".into(),
            identity.incarnation_hex(),
        ),
        (
            "PERSIST_STATE_HELPER".into(),
            helper.to_string_lossy().into_owned(),
        ),
        (STATE_ENV_INCLUDE.into(), policy_hint.include.clone()),
        (
            STATE_ENV_MAX_VARIABLES.into(),
            policy_hint.max_variables.clone(),
        ),
        (STATE_ENV_MAX_BYTES.into(), policy_hint.max_bytes.clone()),
        (
            STATE_ENV_POLICY_FINGERPRINT.into(),
            policy_hint.fingerprint.clone(),
        ),
    ]
}

fn shell_name(shell: &str) -> Option<&str> {
    Path::new(shell).file_name()?.to_str()
}

fn private_hook_dir(history_dir: &Path, session_id: u32) -> Result<PathBuf> {
    let path = history_dir.join(".hooks").join(session_id.to_string());
    fs::create_dir_all(&path).map_err(|source| io_error("create shell hook directory", source))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
        .map_err(|source| io_error("set shell hook directory permissions", source))?;
    Ok(path)
}

fn write_private(path: &Path, content: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|source| io_error("create shell hook", source))?;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|source| io_error("set shell hook permissions", source))?;
    file.write_all(content.as_bytes())
        .map_err(|source| io_error("write shell hook", source))
}

fn io_error(operation: &'static str, source: std::io::Error) -> PersistError {
    PersistError::Io { operation, source }
}

const BASH_RC: &str = r#"if [[ -r "$HOME/.bashrc" ]]; then
    source "$HOME/.bashrc"
fi
__persist_state_sequence=0
__persist_state_shell_pid=${BASHPID:-$$}
__persist_state_commit() {
    [[ ${BASHPID:-$$} == "$__persist_state_shell_pid" ]] || return 0
    ((__persist_state_sequence+=1))
    printf '%s' "$PWD" | PERSIST_STATE_SEQUENCE=$__persist_state_sequence \
        command "$PERSIST_STATE_HELPER" __state-commit >/dev/null 2>&1 || :
    return 0
}
__persist_state_exit() {
    local __persist_exit_status=$?
    __persist_state_commit
    return "$__persist_exit_status"
}
if [[ -z $(trap -p EXIT) ]]; then
    trap '__persist_state_exit' EXIT
else
    printf 'exit-conflict\n' >"$PERSIST_STATE_STATUS"
    chmod 600 "$PERSIST_STATE_STATUS" 2>/dev/null || :
fi
__persist_history_line=$(HISTTIMEFORMAT= builtin history 1 2>/dev/null) || __persist_history_line=
__persist_history_line=${__persist_history_line#"${__persist_history_line%%[![:space:]]*}"}
__persist_history_last=${__persist_history_line%%[!0-9]*}
__persist_history_last=${__persist_history_last:-0}
__persist_history_capture() {
    local __persist_history_line __persist_current
    local __persist_command
    __persist_history_line=$(HISTTIMEFORMAT= builtin history 1 2>/dev/null) || __persist_history_line=
    __persist_history_line=${__persist_history_line#"${__persist_history_line%%[![:space:]]*}"}
    __persist_current=${__persist_history_line%%[!0-9]*}
    __persist_current=${__persist_current:-0}
    if [[ $__persist_current != "$__persist_history_last" ]]; then
        __persist_command=${__persist_history_line#"$__persist_current"}
        __persist_command=${__persist_command#  }
        if [[ -n $__persist_command ]]; then
            printf '%s' "$__persist_command" | command "$PERSIST_HISTORY_HELPER" \
                __history-append "$PERSIST_SESSION_ID" bash >/dev/null 2>&1 || :
        fi
        __persist_history_last=$__persist_current
    fi
    return 0
}
if declare -p PROMPT_COMMAND 2>/dev/null | command grep -q '^declare -a'; then
    PROMPT_COMMAND+=(__persist_history_capture)
    PROMPT_COMMAND+=(__persist_state_commit)
elif [[ -n ${PROMPT_COMMAND:-} ]]; then
    PROMPT_COMMAND="${PROMPT_COMMAND};__persist_history_capture;__persist_state_commit"
else
    PROMPT_COMMAND="__persist_history_capture;__persist_state_commit"
fi
"#;

const ZSH_ENV: &str = r#"__persist_user_zdotdir=${PERSIST_ORIGINAL_ZDOTDIR:-$HOME}
if [[ -r "$__persist_user_zdotdir/.zshenv" ]]; then
    source "$__persist_user_zdotdir/.zshenv"
fi
export ZDOTDIR=$PERSIST_HOOK_DIR
"#;

const ZSH_PROFILE: &str = r#"__persist_user_zdotdir=${PERSIST_ORIGINAL_ZDOTDIR:-$HOME}
if [[ -r "$__persist_user_zdotdir/.zprofile" ]]; then
    source "$__persist_user_zdotdir/.zprofile"
fi
export ZDOTDIR=$PERSIST_HOOK_DIR
"#;

const ZSH_RC: &str = r#"__persist_user_zdotdir=${PERSIST_ORIGINAL_ZDOTDIR:-$HOME}
if [[ -r "$__persist_user_zdotdir/.zshrc" ]]; then
    source "$__persist_user_zdotdir/.zshrc"
fi
typeset -gi __persist_state_sequence=0
__persist_state_commit() {
    (( ZSH_SUBSHELL == 0 )) || return 0
    ((__persist_state_sequence+=1))
    print -rn -- "$PWD" | PERSIST_STATE_SEQUENCE=$__persist_state_sequence \
        command "$PERSIST_STATE_HELPER" __state-commit >/dev/null 2>&1 || true
    return 0
}
__persist_state_exit() {
    local __persist_exit_status=$?
    __persist_state_commit
    return $__persist_exit_status
}
autoload -Uz add-zsh-hook
add-zsh-hook precmd __persist_state_commit
add-zsh-hook zshexit __persist_state_exit
typeset -g __persist_history_last=${HISTCMD:-0}
__persist_history_capture() {
    local __persist_current=${HISTCMD:-0}
    local __persist_command
    if (( __persist_current > __persist_history_last )); then
        __persist_command=$(builtin fc -ln -1 2>/dev/null) || __persist_command=
        __persist_command=${__persist_command#$'\t '}
        if [[ -n $__persist_command ]]; then
            print -rn -- "$__persist_command" | command "$PERSIST_HISTORY_HELPER" \
                __history-append "$PERSIST_SESSION_ID" zsh >/dev/null 2>&1 || true
        fi
        __persist_history_last=$__persist_current
    fi
    return 0
}
if (( ! $+functions[zshaddhistory] && ${#zshaddhistory_functions} == 0 )) \
    && [[ ! -o HIST_IGNORE_ALL_DUPS && ! -o HIST_IGNORE_DUPS \
        && ! -o HIST_IGNORE_SPACE && ! -o HIST_NO_STORE ]]; then
    autoload -Uz add-zsh-hook
    add-zsh-hook precmd __persist_history_capture
else
    print -r -- filtered >| "$PERSIST_HISTORY_STATUS"
    chmod 600 "$PERSIST_HISTORY_STATUS" 2>/dev/null || true
fi
if [[ ${ZDOTDIR:-} == "$PERSIST_HOOK_DIR" ]]; then
    if [[ $PERSIST_ORIGINAL_ZDOTDIR_SET == 1 ]]; then
        export ZDOTDIR=$PERSIST_ORIGINAL_ZDOTDIR
    else
        unset ZDOTDIR
    fi
fi
"#;

const FISH_INIT: &str = r#"set -g __persist_state_sequence 0
function __persist_state_commit
    set -g __persist_state_sequence (math $__persist_state_sequence + 1)
    printf %s "$PWD" | env PERSIST_STATE_SEQUENCE=$__persist_state_sequence "$PERSIST_STATE_HELPER" __state-commit >/dev/null 2>&1
    return 0
end
function __persist_state_postexec --on-event fish_postexec
    __persist_state_commit
end
function __persist_state_exit --on-event fish_exit
    __persist_state_commit
end
__persist_state_commit
if not functions -q fish_should_add_to_history
    function __persist_history_postexec --on-event fish_postexec
        set -l command_line $argv[1]
        if test -n "$command_line"
            printf %s "$command_line" | command "$PERSIST_HISTORY_HELPER" __history-append "$PERSIST_SESSION_ID" fish >/dev/null 2>&1
        end
    end
else
    printf 'filtered\n' >"$PERSIST_HISTORY_STATUS"
    chmod 600 "$PERSIST_HISTORY_STATUS" >/dev/null 2>&1
end"#;

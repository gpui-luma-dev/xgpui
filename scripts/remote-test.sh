#!/usr/bin/env bash
# Remote test runner for the Windows and Fedora test machines.
#
# One-time setup on a test machine (Git Bash on Windows, any bash on Linux):
#
#   bash <(curl -fsSL https://raw.githubusercontent.com/gpui-luma-dev/xgpui/main/scripts/remote-test.sh) install
#
# Install finds or clones the repository and puts an `xgpui-test` command on PATH.
# After that, every test run is just:
#
#   xgpui-test            pick from open PRs and recent branches
#   xgpui-test 271        test PR #271
#   xgpui-test <branch>   test a branch by name
#
# A run fetches from GitHub, lists open pull requests (branches in this repository
# only, never forks) and recent branches, switches the clone to the one you pick,
# fast-forwards it, shows that branch's `.remote-test.sh` and runs it after you
# press Enter. Output is saved under ~/xgpui-test-logs.
#
# `xgpui-test` always runs the newest copy of this file from origin/main, so it
# keeps working whatever branch the clone is on, and changes here reach the test
# machines without reinstalling.
#
# Writing a branch's .remote-test.sh
# ----------------------------------
# It is a temporary bash script at the repository root of a working branch. It
# runs from the repository root with these helpers:
#
#   note "text"                 print a heading
#   run_example NAME [PACKAGE]  build an example (default package: xgpui) and
#                               start it in the background; stopped when the
#                               script ends
#   stop_example                stop examples started by run_example
#   step "do this" "expect"     one manual check; the tester answers pass, fail
#                               or skip and the answer is recorded
#   CMD | report                show CMD's output and include it in the posted
#                               results (digests, versions; not build output)
#   post_to 271 [...]           post the results to these issues or PRs instead
#                               of the PR under test
#
# When the run ends, the runner posts a comment with the step results and the
# reported output to the PR under test (or the post_to targets), then reads it
# back to confirm it landed. It needs a GitHub token once per machine: GH_TOKEN,
# GITHUB_TOKEN, a logged-in `gh`, or one pasted at the first prompt (saved to
# ~/.config/xgpui-test/github-token, mode 600).
#
# Write each step as one action plus the result it should produce, so that a
# person or an agent on the machine can follow it. The file must be deleted
# before an epic branch merges to main; CI refuses it on main.
#
# Environment:
#   XGPUI_REPO                 clone to use (set by the installed command)
#   XGPUI_TEST_NONINTERACTIVE  1 = never prompt; dirty clones and rewritten
#                              branches stop the run instead of asking, and
#                              steps are recorded as TODO
#   XGPUI_TEST_LOG_DIR         log directory (default ~/xgpui-test-logs)

set -uo pipefail

REPO_SLUG=gpui-luma-dev/xgpui
REPO_URL="https://github.com/$REPO_SLUG.git"
TEST_SCRIPT=.remote-test.sh
BRANCH_LIMIT=15

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

interactive() {
    [[ "${XGPUI_TEST_NONINTERACTIVE:-0}" != 1 ]]
}

ask() {
    # ask "prompt" -> answer in $REPLY ("" when non-interactive)
    REPLY=""
    if interactive; then
        read -r -p "$1" REPLY || REPLY=""
    fi
}

usage() {
    cat <<'USAGE'
xgpui-test            pick from open PRs and recent branches, then run its .remote-test.sh
xgpui-test 271        test PR #271
xgpui-test <branch>   test a branch by name
remote-test.sh install [CLONE]   set up the xgpui-test command on this machine
USAGE
}

# --------------------------------------------------------------------------
# install

is_repo_clone() {
    local url
    url=$(git -C "$1" remote get-url origin 2>/dev/null) || return 1
    [[ "$url" == *"$REPO_SLUG"* ]]
}

pick_bin_dir() {
    local dir
    for dir in "$HOME/.local/bin" "$HOME/bin"; do
        case ":$PATH:" in *":$dir:"*) printf '%s\n' "$dir"; return ;; esac
    done
    # Neither is on PATH yet. Git Bash adds ~/bin when it exists; elsewhere we
    # add it to ~/.bashrc below.
    printf '%s\n' "$HOME/bin"
}

cmd_install() {
    command -v git >/dev/null || die "git is not installed"
    command -v curl >/dev/null || die "curl is not installed"

    local repo="${1:-}"
    if [[ -z "$repo" ]]; then
        if is_repo_clone "$PWD"; then
            repo=$(git -C "$PWD" rev-parse --show-toplevel)
        else
            local guess
            for guess in "$HOME/xgpui" "$HOME/Developer/xgpui" "$HOME/src/xgpui" "$HOME/GitHub/xgpui"; do
                if is_repo_clone "$guess"; then repo=$guess; break; fi
            done
        fi
    fi
    if [[ -z "$repo" ]]; then
        read -r -p "Path to your xgpui clone (Enter to clone into $HOME/xgpui): " repo
        repo=${repo:-$HOME/xgpui}
    fi
    repo=${repo/#\~/$HOME}

    if [[ ! -d "$repo/.git" ]]; then
        [[ -e "$repo" ]] && die "$repo exists but is not a git clone"
        echo "Cloning $REPO_URL into $repo ..."
        git clone "$REPO_URL" "$repo" || die "clone failed"
    fi
    is_repo_clone "$repo" || die "$repo is not a clone of $REPO_SLUG"
    repo=$(cd "$repo" && pwd)

    local bin_dir launcher
    bin_dir=$(pick_bin_dir)
    mkdir -p "$bin_dir"
    launcher="$bin_dir/xgpui-test"

    # The launcher stays tiny on purpose: everything else lives in this file on
    # origin/main and updates itself.
    cat >"$launcher" <<LAUNCHER
#!/usr/bin/env bash
# Installed by scripts/remote-test.sh install. Rerun install to change the clone.
export XGPUI_REPO='$repo'
cd "\$XGPUI_REPO" || { echo "error: \$XGPUI_REPO is missing; rerun install" >&2; exit 1; }
git fetch --quiet --prune origin || echo "warning: fetch failed; using the last fetched copy" >&2
runner_ref=origin/main
if ! git cat-file -e "\$runner_ref:scripts/remote-test.sh" 2>/dev/null; then
    # Before the runner reaches main, use the newest branch that has it.
    runner_ref=
    while read -r ref; do
        if git cat-file -e "\$ref:scripts/remote-test.sh" 2>/dev/null; then runner_ref=\$ref; break; fi
    done < <(git for-each-ref --sort=-committerdate --format='%(refname)' refs/remotes/origin)
    [[ -n "\$runner_ref" ]] || { echo "error: no branch on origin has scripts/remote-test.sh" >&2; exit 1; }
fi
runner=\$(mktemp "\${TMPDIR:-/tmp}/xgpui-test.XXXXXX") || exit 1
trap 'rm -f "\$runner"' EXIT
git show "\$runner_ref:scripts/remote-test.sh" >"\$runner" || exit 1
XGPUI_RUNNER_REF=\$runner_ref bash "\$runner" run "\$@"
LAUNCHER
    chmod +x "$launcher"

    case ":$PATH:" in
        *":$bin_dir:"*) ;;
        *)
            local rc="$HOME/.bashrc" line="export PATH=\"$bin_dir:\$PATH\"  # xgpui-test"
            if ! grep -qF '# xgpui-test' "$rc" 2>/dev/null; then
                printf '\n%s\n' "$line" >>"$rc"
            fi
            echo "Added $bin_dir to PATH in $rc. Open a new terminal before the first run."
            ;;
    esac

    echo "Installed $launcher"
    echo "Clone: $repo"
    echo "Run tests with: xgpui-test"
}

# --------------------------------------------------------------------------
# listing

# Prints "number<TAB>branch" for open PRs whose branch is in this repository.
# Uses only curl, grep and git. Fork PRs are dropped because their head commit is
# not the tip of any branch here: the runner executes whatever the branch contains.
list_prs() {
    local json open sha ref number name
    json=$(curl -fsSL --max-time 20 -H 'Accept: application/vnd.github+json' \
        "https://api.github.com/repos/$REPO_SLUG/pulls?state=open&per_page=100") ||
        { echo "note: could not reach the GitHub API; showing branches only" >&2; return 0; }

    # Open PR numbers, taken from each PR's own "html_url" field. Text inside a PR
    # body is escaped (\"html_url\"), so it cannot match.
    open=$(grep -oE "\"html_url\": *\"https://github.com/$REPO_SLUG/pull/[0-9]+\"" <<<"$json" |
        grep -oE '[0-9]+"$' | tr -d '"' | sort -un)
    [[ -n "$open" ]] || return 0

    # Branch tips on origin, by commit.
    local -A tips=()
    while read -r sha ref; do
        name=${ref#refs/remotes/origin/}
        [[ "$name" == HEAD ]] && continue
        tips[$sha]="${tips[$sha]:-}${tips[$sha]:+ }$name"
    done < <(git for-each-ref --format='%(objectname) %(refname)' refs/remotes/origin)

    # GitHub keeps refs/pull/<n>/head pointing at each PR's head commit.
    while read -r sha ref; do
        number=${ref#refs/pull/}
        number=${number%/head}
        grep -qx "$number" <<<"$open" || continue
        for name in ${tips[$sha]:-}; do
            printf '%s\t%s\n' "$number" "$name"
        done
    done < <(git ls-remote origin 'refs/pull/*/head')
}

has_test_script() {
    git cat-file -e "origin/$1:$TEST_SCRIPT" 2>/dev/null
}

# --------------------------------------------------------------------------
# helpers exported to .remote-test.sh

note() {
    printf '\n== %s\n' "$*"
}

run_example() {
    local name=$1 package=${2:-xgpui} target bin
    note "Building example $name (first build on a branch can take several minutes)"
    cargo build -p "$package" --example "$name" || { echo "build failed" >&2; return 1; }
    target=${CARGO_TARGET_DIR:-target}
    bin="$target/debug/examples/$name"
    [[ -f "$bin.exe" ]] && bin="$bin.exe"
    [[ -f "$bin" ]] || { echo "built example not found at $bin" >&2; return 1; }
    "$bin" &
    XGPUI_EXAMPLE_PIDS="${XGPUI_EXAMPLE_PIDS:-} $!"
    trap stop_example EXIT
    echo "Started $name (pid $!)."
    sleep 2
}

stop_example() {
    local pid
    for pid in ${XGPUI_EXAMPLE_PIDS:-}; do
        kill "$pid" 2>/dev/null
    done
    XGPUI_EXAMPLE_PIDS=""
}

step() {
    local action=$1 expect=${2:-} answer result reason=""
    XGPUI_STEP=$(( ${XGPUI_STEP:-0} + 1 ))
    printf '\n[%d] Do:     %s\n' "$XGPUI_STEP" "$action"
    [[ -n "$expect" ]] && printf '    Expect: %s\n' "$expect"
    if [[ "${XGPUI_TEST_NONINTERACTIVE:-0}" == 1 ]]; then
        result=TODO
    else
        while :; do
            read -r -p "    p = pass, f = fail, s = skip > " answer || answer=s
            case "$answer" in
                p | P) result=PASS; break ;;
                f | F) result=FAIL; read -r -p "    What happened? " reason || reason=""; break ;;
                s | S) result=SKIP; break ;;
            esac
        done
    fi
    printf '    -> %s\n' "$result"
    printf '%s\t%d\t%s\t%s\n' "$result" "$XGPUI_STEP" "$action" "$reason" >>"$XGPUI_TEST_RESULTS"
}

report() {
    tee -a "$XGPUI_TEST_REPORT"
}

post_to() {
    printf '%s\n' "${@#\#}" >>"$XGPUI_TEST_POST_TO"
}

# --------------------------------------------------------------------------
# posting results

TOKEN_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/xgpui-test/github-token"

# Sets XGPUI_GH_TOKEN, asking once per machine when nothing else provides one.
github_auth() {
    XGPUI_GH_TOKEN=${GH_TOKEN:-${GITHUB_TOKEN:-}}
    [[ -n "$XGPUI_GH_TOKEN" ]] && return 0
    if [[ -s "$TOKEN_FILE" ]]; then
        XGPUI_GH_TOKEN=$(tr -d '[:space:]' <"$TOKEN_FILE")
        [[ -n "$XGPUI_GH_TOKEN" ]] && return 0
    fi
    if command -v gh >/dev/null && XGPUI_GH_TOKEN=$(gh auth token 2>/dev/null) && [[ -n "$XGPUI_GH_TOKEN" ]]; then
        return 0
    fi
    interactive || return 1
    cat <<EOF

Posting results to GitHub needs a token, asked for once on this machine.
Create one at https://github.com/settings/personal-access-tokens/new
  repository access: only $REPO_SLUG
  permissions: Issues (read and write), Pull requests (read and write)
It is saved to $TOKEN_FILE (readable only by you).
EOF
    local token
    read -r -s -p "Paste the token (Enter to skip posting this time): " token || token=""
    echo
    token=$(tr -d '[:space:]' <<<"$token")
    [[ -n "$token" ]] || return 1
    mkdir -p "$(dirname "$TOKEN_FILE")"
    (umask 077 && printf '%s\n' "$token" >"$TOKEN_FILE")
    XGPUI_GH_TOKEN=$token
}

# json_string FILE: the file's text as one JSON string.
json_string() {
    awk 'BEGIN { printf "\"" }
         {
             gsub(/\\/, "\\\\"); gsub(/"/, "\\\""); gsub(/\t/, "    ")
             gsub(/[[:cntrl:]]/, "")
             printf "%s\\n", $0
         }
         END { printf "\"" }' "$1"
}

# post_comment NUMBER BODY_FILE MARKER: post, read back, print the comment's URL.
post_comment() {
    local number=$1 body=$2 marker=$3 payload response api url
    payload=$(mktemp "${TMPDIR:-/tmp}/xgpui-comment.XXXXXX") || return 1
    { printf '{"body":'; json_string "$body"; printf '}'; } >"$payload"
    response=$(curl -sS --max-time 30 -X POST \
        -H "Authorization: Bearer $XGPUI_GH_TOKEN" -H 'Accept: application/vnd.github+json' \
        --data-binary @"$payload" "https://api.github.com/repos/$REPO_SLUG/issues/$number/comments")
    rm -f "$payload"
    api=$(grep -oE '"url": *"https://api\.github\.com/repos/[^"]+/issues/comments/[0-9]+"' <<<"$response" |
        head -1 | grep -oE 'https://[^"]+')
    url=$(grep -oE '"html_url": *"https://github\.com/[^"]+#issuecomment-[0-9]+"' <<<"$response" |
        head -1 | grep -oE 'https://[^"]+')
    if [[ -z "$api" || -z "$url" ]]; then
        echo "posting to #$number failed: $(grep -oE '"message": *"[^"]*"' <<<"$response" | head -1)" >&2
        return 1
    fi
    if ! curl -fsS --max-time 20 -H "Authorization: Bearer $XGPUI_GH_TOKEN" \
        -H 'Accept: application/vnd.github+json' "$api" | grep -qF "$marker"; then
        echo "posted to #$number but could not read it back: $url" >&2
        return 1
    fi
    printf '%s\n' "$url"
}

# post_results BRANCH HASH STATUS RESULTS REPORT POST_TO PR LOG
post_results() {
    local branch=$1 hash=$2 status=$3 results=$4 report=$5 post_to_file=$6 pr=$7 log=$8
    local -a targets=()
    local number
    if [[ -s "$post_to_file" ]]; then
        while read -r number; do
            [[ "$number" =~ ^[0-9]+$ ]] && targets+=("$number")
        done < <(sort -un "$post_to_file")
    elif [[ -n "$pr" ]]; then
        targets=("$pr")
    fi
    if ((${#targets[@]} == 0)); then
        echo "No PR or issue to post to (the branch has no open PR and the script named none)."
        return 0
    fi

    local marker body result n action reason
    marker="xgpui-test run $(basename "${log%.log}")"
    body=$(mktemp "${TMPDIR:-/tmp}/xgpui-body.XXXXXX") || return 1
    {
        echo "### Remote test: \`$branch\` @ \`$hash\`"
        echo
        echo "machine: \`$(uname -srm)\` · $(date '+%Y-%m-%d %H:%M %z') · $marker"
        echo
        if [[ -s "$results" ]]; then
            while IFS=$'\t' read -r result n action reason; do
                echo "- **$result** [$n] $action${reason:+ — _${reason}_}"
            done <"$results"
        else
            echo "(no manual steps recorded)"
        fi
        echo
        echo "script exit status: $status"
        if [[ -s "$report" ]]; then
            echo
            echo '~~~~text'
            head -c 60000 "$report"
            [[ $(wc -c <"$report") -gt 60000 ]] && printf '\n[report cut at 60000 bytes; full log on the machine: %s]\n' "$log"
            echo '~~~~'
        fi
    } >"$body"

    note "Posting results to GitHub"
    if ! github_auth; then
        echo "Not posted (no GitHub token). The results are in $log"
        rm -f "$body"
        return 1
    fi
    local url failed=0
    for number in "${targets[@]}"; do
        if url=$(post_comment "$number" "$body" "$marker"); then
            echo "posted to #$number: $url"
        else
            failed=1
        fi
    done
    rm -f "$body"
    ((failed == 0)) || echo "Some posts failed; the results are in $log"
    return "$failed"
}

# --------------------------------------------------------------------------
# run

switch_to() {
    local branch=$1
    if git show-ref --verify --quiet "refs/heads/$branch"; then
        git switch --quiet "$branch" || die "could not switch to $branch"
    else
        git switch --quiet --create "$branch" --track "origin/$branch" || die "could not switch to $branch"
    fi
    if ! git merge --quiet --ff-only "origin/$branch" 2>/dev/null; then
        echo "The local $branch has diverged from GitHub (the branch was probably rewritten)."
        ask "Reset it to the GitHub version? Local commits on $branch on this machine are dropped. [y/N] "
        [[ "$REPLY" == [yY]* ]] || die "left $branch as it was"
        git reset --quiet --hard "origin/$branch" || die "reset failed"
    fi
}

cmd_run() {
    local repo=${XGPUI_REPO:-}
    [[ -n "$repo" ]] || repo=$(git rev-parse --show-toplevel 2>/dev/null) || die "run inside the xgpui clone or use xgpui-test"
    cd "$repo" || die "cannot enter $repo"
    is_repo_clone . || die "$repo is not a clone of $REPO_SLUG"

    # Local edits would block or be carried across the switch.
    local dirty
    dirty=$(git status --porcelain --untracked-files=no)
    if [[ -n "$dirty" ]]; then
        echo "This clone has uncommitted changes:"
        printf '%s\n' "$dirty"
        ask "Stash them and continue? [y/N] "
        [[ "$REPLY" == [yY]* ]] || die "stopped; the clone is unchanged"
        git stash push --quiet --message "xgpui-test $(date +%Y-%m-%d_%H:%M:%S)" || die "stash failed"
        echo "Stashed. Recover with: git stash pop"
    fi

    # The launcher already fetched; a direct `bash scripts/remote-test.sh` has not.
    [[ -n "${XGPUI_RUNNER_REF:-}" ]] || git fetch --quiet --prune origin || echo "warning: fetch failed" >&2

    local -a choices=() labels=()
    local -A seen=()
    local number branch mark
    while IFS=$'\t' read -r number branch; do
        [[ -n "$number" ]] || continue
        git show-ref --verify --quiet "refs/remotes/origin/$branch" || continue
        seen[$branch]=$number
        mark=""; has_test_script "$branch" && mark=" [test]"
        choices+=("$branch")
        labels+=("$(printf 'PR #%-5s %s%s\n         %s' "$number" "$branch" "$mark" \
            "$(git log -1 --format=%s "origin/$branch")")")
    done < <(list_prs)

    local count=0 when subject
    while IFS=$'\t' read -r branch when subject; do
        [[ "$branch" == HEAD || -n "${seen[$branch]:-}" ]] && continue
        (( count < BRANCH_LIMIT )) || [[ "$branch" == main ]] || continue
        count=$((count + 1))
        mark=""; has_test_script "$branch" && mark=" [test]"
        choices+=("$branch")
        labels+=("$(printf 'branch   %s%s (%s)\n         %s' "$branch" "$mark" "$when" "$subject")")
    done < <(git for-each-ref --sort=-committerdate \
        --format=$'%(refname:lstrip=3)\t%(committerdate:relative)\t%(subject)' refs/remotes/origin)

    ((${#choices[@]})) || die "nothing to test: no PRs or branches found on origin"

    local pick=${1:-} target="" i
    if [[ -n "$pick" ]]; then
        pick=${pick#\#}
        for i in "${!choices[@]}"; do
            if [[ "${choices[$i]}" == "$pick" || "${seen[${choices[$i]}]:-}" == "$pick" ]]; then
                target=${choices[$i]}
                break
            fi
        done
        [[ -n "$target" ]] || die "no open PR or branch matches '$1'"
    else
        echo
        echo "Current: $(git branch --show-current) @ $(git rev-parse --short HEAD)"
        echo "[test] = the branch has a $TEST_SCRIPT"
        echo
        for i in "${!choices[@]}"; do
            printf '%3d) %s\n' "$((i + 1))" "${labels[$i]}"
        done
        echo
        interactive || die "no PR or branch given (non-interactive)"
        read -r -p "Pick a number (q to quit): " pick || exit 0
        [[ "$pick" == q* || -z "$pick" ]] && exit 0
        # shellcheck disable=SC2015 # die only runs when a test fails
        [[ "$pick" =~ ^[0-9]+$ ]] && ((pick >= 1 && pick <= ${#choices[@]})) || die "not a choice: $pick"
        target=${choices[$((pick - 1))]}
    fi

    switch_to "$target"
    local hash
    hash=$(git rev-parse --short HEAD)
    echo
    echo "testing: $target @ $hash"

    if ! git cat-file -e "HEAD:$TEST_SCRIPT" 2>/dev/null; then
        echo "This branch has no $TEST_SCRIPT, so there is nothing to run."
        echo "The clone is now on $target @ $hash."
        exit 0
    fi

    # Run the committed copy (LF endings, no local edits) from a temp file.
    local script
    script=$(mktemp "${TMPDIR:-/tmp}/xgpui-branch-test.XXXXXX") || die "mktemp failed"
    git show "HEAD:$TEST_SCRIPT" >"$script" || die "could not read $TEST_SCRIPT"

    echo
    echo "----- $TEST_SCRIPT -----"
    awk '{ printf "%4d  %s\n", NR, $0 }' "$script"
    echo "------------------------"
    if interactive; then
        read -r -p "Press Enter to run it, q to quit: " REPLY || exit 0
        [[ "$REPLY" == q* ]] && { rm -f "$script"; exit 0; }
    fi

    local log_dir=${XGPUI_TEST_LOG_DIR:-$HOME/xgpui-test-logs} log results report post_to_file status
    mkdir -p "$log_dir"
    log="$log_dir/$(date +%Y%m%d-%H%M%S)-${target//\//-}-$hash.log"
    results=$(mktemp "${TMPDIR:-/tmp}/xgpui-results.XXXXXX") || die "mktemp failed"
    report=$(mktemp "${TMPDIR:-/tmp}/xgpui-report.XXXXXX") || die "mktemp failed"
    post_to_file=$(mktemp "${TMPDIR:-/tmp}/xgpui-post-to.XXXXXX") || die "mktemp failed"
    {
        echo "testing: $target @ $hash"
        echo "machine: $(uname -srm)"
        echo "date:    $(date '+%Y-%m-%d %H:%M:%S %z')"
        echo "runner:  ${XGPUI_RUNNER_REF:-local copy}"
    } | tee "$log"

    export XGPUI_TEST_RESULTS=$results XGPUI_TEST_LOG=$log
    export XGPUI_TEST_REPORT=$report XGPUI_TEST_POST_TO=$post_to_file XGPUI_TEST_POSTS=1
    export -f note run_example stop_example step report post_to
    bash "$script" 2>&1 | tee -a "$log"
    status=${PIPESTATUS[0]}

    {
        echo
        echo "===== summary: $target @ $hash ====="
        local result n action reason fails=0
        while IFS=$'\t' read -r result n action reason; do
            printf '%-4s [%s] %s%s\n' "$result" "$n" "$action" "${reason:+ -- $reason}"
            [[ "$result" == FAIL ]] && fails=$((fails + 1))
        done <"$results"
        [[ -s "$results" ]] || echo "(no steps recorded)"
        echo "script exit status: $status"
        echo "log: $log"
        ((fails == 0 && status == 0))
    } 2>&1 | tee -a "$log"
    local summary_status=${PIPESTATUS[0]}
    post_results "$target" "$hash" "$status" "$results" "$report" "$post_to_file" \
        "${seen[$target]:-}" "$log" 2>&1 | tee -a "$log"
    rm -f "$script" "$results" "$report" "$post_to_file"
    exit "$summary_status"
}

main() {
    ((BASH_VERSINFO[0] >= 4)) || die "bash 4 or newer is required (this is bash $BASH_VERSION)"
    case "${1:-run}" in
        install) shift; cmd_install "$@" ;;
        run) shift; cmd_run "$@" ;;
        -h | --help | help) usage ;;
        *) cmd_run "$@" ;;
    esac
}

# Sourcing defines the helpers without running anything (used by branch scripts).
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi

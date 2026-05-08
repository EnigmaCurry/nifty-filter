#!/bin/bash

stderr(){ echo "$@" >/dev/stderr; }
error(){ stderr "Error: $@"; }
fault(){ test -n "$1" && error $1; stderr "Exiting."; exit 1; }
cancel(){ stderr "Canceled."; exit 2; }
exe() { (set -x; "$@"); }
print_array(){ printf '%s\n' "$@"; }
trim_trailing_whitespace() { sed -e 's/[[:space:]]*$//'; }
trim_leading_whitespace() { sed -e 's/^[[:space:]]*//'; }
trim_whitespace() { trim_leading_whitespace | trim_trailing_whitespace; }
check_var(){
    local __missing=false
    local __vars="$@"
    for __var in ${__vars}; do
        if [[ -z "${!__var}" ]]; then
            error "${__var} variable is missing."
            __missing=true
        fi
    done
    if [[ ${__missing} == true ]]; then
        fault
    fi
}

check_num(){
    local var=$1
    check_var var
    if ! [[ ${!var} =~ ^[0-9]+$ ]] ; then
        fault "${var} is not a number: '${!var}'"
    fi
}

debug_var() {
    local var=$1
    check_var var
    stderr "## DEBUG: ${var}=${!var}"
}

debug_array() {
    local -n ary=$1
    echo "## DEBUG: Array '$1' contains:"
    for i in "${!ary[@]}"; do
        echo "## ${i} = ${ary[$i]}"
    done
}

ask() {
    ## Ask the user a question and set the given variable name with their answer
    local __prompt="${1}"; local __var="${2}"; local __default="${3}"
    read -e -p "${__prompt}"$'\x0a: ' -i "${__default}" ${__var}
    export ${__var}
}

ask_no_blank() {
    ## Ask the user a question and set the given variable name with their answer
    ## If the answer is blank, repeat the question.
    local __prompt="${1}"; local __var="${2}"; local __default="${3}"
    while true; do
        read -e -p "${__prompt}"$'\x0a: ' -i "${__default}" ${__var}
        export ${__var}
        [[ -z "${!__var}" ]] || break
    done
}

ask_echo() {
    ## Ask the user a question then print the non-blank answer to stdout
    (
        ask_no_blank "$1" ASK_ECHO_VARNAME >/dev/stderr
        echo "${ASK_ECHO_VARNAME}"
    )
}

require_input() {
    ## require_input {PROMPT} {VAR} {DEFAULT}
    ## Read variable, set default if blank, error if still blank
    test -z ${3} && dflt="" || dflt=" (${3})"
    read -e -p "$1$dflt: " $2
    eval $2=${!2:-${3}}
    test -v ${!2} && fault "$2 must not be blank."
}

make_var_name() {
    # Make an environment variable out of any string
    # Replaces all invalid characters with a single _
    echo "$@" | sed -e 's/  */_/g' -e 's/--*/_/g' -e 's/[^a-zA-Z0-9_]/_/g' -e 's/__*/_/g' -e 's/.*/\U&/' -e 's/__*$//' -e 's/^__*//'
}

confirm() {
    ## Confirm with the user.
    local default=$1; local prompt=$2; local question=${3:-". Proceed?"}
    check_var default prompt question
    if [[ $default == "y" || $default == "yes" || $default == "ok" ]]; then
        dflt="Y/n"
    else
        dflt="y/N"
    fi
    read -e -p "${prompt}${question} (${dflt}): " answer
    answer=${answer:-${default}}
    if [[ ${answer,,} == "y" || ${answer,,} == "yes" || ${answer,,} == "ok" ]]; then
        return 0
    else
        return 1
    fi
}
check_deps() {
    missing=""
    for var in "$@"; do
        echo -n "Looking for ${var} ... " >/dev/stderr
        if ! command -v "${var}" >/dev/null 2>&1; then
            echo "Missing! No ${var} found in PATH." >/dev/stderr
            missing="${missing} ${var}"
        else
            echo found $(which "${var}")
        fi
    done

    if [[ -n "${missing}" ]]; then fault "Missing dependencies: ${missing}"; fi
}
check_emacs_unsaved_files() {
    lock_files=$(find . -name ".#*")
    if [ ! -z "$lock_files" ]; then
        echo "Warning: You have unsaved files in Emacs. Please save all files before building."
        echo "Unsaved files:"
        echo "$lock_files"
        return 1
    fi
}

template_changelog_files() {
  set -euo pipefail
  local base
  base="$(git rev-list --reverse --first-parent HEAD | head -n1)"

  # ------------------------------------------------------------------
  # 1️⃣  Detect pure moves/copies out of the `template/` tree.
  # ------------------------------------------------------------------
  local -A skip=()
  while IFS=$'\t' read -r status old new; do
    case "$status" in
      R100|C100)
        [[ "$old" == template/* ]] && skip["$new"]=1
        ;;
    esac
  done < <(git diff --name-status -M --diff-filter=RC "${base}..HEAD")

  # ------------------------------------------------------------------
  # 2️⃣  Files we always want to ignore, no matter what.
  # ------------------------------------------------------------------
  # (you can add more entries here, e.g. LICENSE, CONTRIBUTING.md, …)
  local -a always_skip=( README.md DEVELOPMENT.md )

  # ------------------------------------------------------------------
  # 3️⃣  Walk the changed‑file list, apply the various filters.
  # ------------------------------------------------------------------
  git diff --name-only --diff-filter=AMCR "${base}..HEAD" \
    | LC_ALL=C sort -u \
    | while IFS= read -r f; do

        # ── a)  Skip pure moves/copies from the template tree
        [[ -n "${skip[$f]:-}" ]] && continue

        # ── b)  Skip the hard‑coded list from step 2
        for ignore in "${always_skip[@]}"; do
          [[ "$f" == "$ignore" ]] && continue 2   # ‘continue 2’ jumps out of the for‑loop *and* the while‑loop
        done

        # ── c)  Find the path we have to compare against in the base commit.
        #      Handles both “template/foo → foo” and “foo unchanged”.
        local base_path=""
        if git cat-file -e "${base}:${f}" 2>/dev/null; then
          base_path="$f"
        elif git cat-file -e "${base}:template/${f}" 2>/dev/null; then
          base_path="template/${f}"
        fi

        # ── d)  If the file does not exist in the base commit at either location,
        #      it is a brand‑new file → always include it.
        if [[ -z "$base_path" ]]; then
          printf '%s\n' "$f"
          continue
        fi

        # ── e)  Special handling for .env‑dist (your original logic)
        case "$f" in
          .env-dist)
            if (diff -U0 <(git show "${base}:${base_path}") "$f" || true) \
              | awk '
                  /^--- /    { next }
                  /^\+\+\+ / { next }
                  /^@@/      { next }
                  /^[+-]/ {
                    line = substr($0, 2)
                    if (line ~ /^(ROOT_DIR|DOCKER_IMAGE)=/) next
                    exit 1
                  }
                  END { exit 0 }
                '
            then
              continue
            fi
            ;;
        esac

        # ── f)  If we got here the file is “interesting” → emit it.
        printf '%s\n' "$f"
      done
}

template_diff() {
  set -euo pipefail
  local base
  base="$(git rev-list --reverse --first-parent HEAD | head -n1)"

  # If paths are provided, treat them as a whitelist, but also include template/… paths
  if (($#)); then
    local -A seen=()
    local -a paths=()
    local p alt

    for p in "$@"; do
      # include exactly what user typed
      if [[ -z "${seen[$p]:-}" ]]; then paths+=("$p"); seen["$p"]=1; fi

      # include the "other side" so renames/moves can be shown
      if [[ "$p" == template/* ]]; then
        alt="${p#template/}"        # template/foo -> foo
      else
        alt="template/$p"           # foo -> template/foo
      fi
      if [[ -z "${seen[$alt]:-}" ]]; then paths+=("$alt"); seen["$alt"]=1; fi

      # Optional: if you export APP, also map APP/... to template/PROJECT/...
      if [[ -n "${APP:-}" && "$p" == "axum-dev/"* ]]; then
        alt="template/PROJECT/${p#axum-dev/}"
        if [[ -z "${seen[$alt]:-}" ]]; then paths+=("$alt"); seen["$alt"]=1; fi
      fi
    done

      git -c color.ui=always diff -M "${base}..HEAD" -- "${paths[@]}" | less -RSX
  else
      git -c color.ui=always diff -M "${base}..HEAD" | less -RSX
  fi
}

check_git_clean() {
    local d=$(realpath ${1:-.}) # Default current directory
    [ -d "$d" ] || { fault "Not a directory: $d"; return 2; }
    (
        cd "$d" || { fault "Cannot cd into $d"; exit 1; }
        git rev-parse --git-dir >/dev/null 2>&1 ||
            { fault "Not a git repository: $d"; exit 1; }
        [[ -z "$(git status --porcelain)" ]] ||
            { fault "Working tree is dirty: $d"; exit 1; }
        printf '✔ Repository is clean: %s\n' "$d"
        exit 0
    )
    return $?
}

fresh_template_branch() {
    set -euo pipefail
    export TMP_REMOTE="tmp-import-remote"

    # -----------------------------------------------------------------
    # Helper: remove the temporary remote (used by the trap and by us)
    # -----------------------------------------------------------------
    _clean_tmp_remote() {
        git remote get-url "${TMP_REMOTE}" >/dev/null 2>&1 && \
            git remote remove "${TMP_REMOTE}" 2>/dev/null || true
    }

    # Ensure the temporary remote is removed even if we exit early
    trap '_clean_tmp_remote' EXIT

    # -----------------------------------------------------------------
    # Sanity checks
    # -----------------------------------------------------------------
    check_git_clean

    # -----------------------------------------------------------------
    # Get a name for the new orphan branch
    # -----------------------------------------------------------------
    ask_no_blank "Enter a name for the new orphan branch" NEW_ORPHAN_BRANCH ""
    debug_var NEW_ORPHAN_BRANCH

    # Abort if the branch already exists (local or remote)
    if git show-ref --verify --quiet "refs/heads/${NEW_ORPHAN_BRANCH}" \
        || git ls-remote --heads . "${NEW_ORPHAN_BRANCH}" | grep -q "${NEW_ORPHAN_BRANCH}"; then
        fault "Branch '${NEW_ORPHAN_BRANCH}' already exists."
    fi

    # -----------------------------------------------------------------
    # Create a clean orphan branch
    # -----------------------------------------------------------------
    exe git checkout --orphan "${NEW_ORPHAN_BRANCH}"
    exe git rm -rf .
    exe git clean -fdx >/dev/null

    # -----------------------------------------------------------------
    # Make sure the temporary remote is gone, then add it again
    # -----------------------------------------------------------------
    _clean_tmp_remote
    : "${REMOTE:=https://github.com/EnigmaCurry/rust-axum-template.git}"
    debug_var REMOTE
    exe git remote add "${TMP_REMOTE}" "${REMOTE}"
    exe git fetch "${TMP_REMOTE}"

    # -----------------------------------------------------------------
    # Verify the remote branch exists and capture its SHA‑1
    # -----------------------------------------------------------------
    : "${BRANCH:=master}"
    debug_var BRANCH
    if ! git rev-parse --verify "${TMP_REMOTE}/${BRANCH}" >/dev/null 2>&1; then
        fault "Remote '${REMOTE}' does not contain branch '${BRANCH}'."
    fi
    # The SHA‑1 we are about to squash
    REMOTE_SHA=$(git rev-parse "${TMP_REMOTE}/${BRANCH}")

    # -----------------------------------------------------------------
    # Bring the remote snapshot into the empty work‑tree
    # -----------------------------------------------------------------
    exe git checkout "${TMP_REMOTE}/${BRANCH}" -- .

    # -----------------------------------------------------------------
    # Stage everything and create a **single** commit.
    # The message now contains remote, branch and SHA.
    # -----------------------------------------------------------------
    exe git add -A
    COMMIT_MSG="init: ${REMOTE} ${BRANCH} ${REMOTE_SHA}"
    exe git commit -m "${COMMIT_MSG}"

    # -----------------------------------------------------------------
    # Sanity‑check that we really have exactly one commit
    # -----------------------------------------------------------------
    [[ $(git rev-list --count HEAD) -eq 1 ]] || fault "Unexpected commit count."

    # -----------------------------------------------------------------
    # Clean up the temporary remote (the trap will also do this on EXIT)
    # -----------------------------------------------------------------
    exe git remote remove "${TMP_REMOTE}"

    # -----------------------------------------------------------------
    # Success message
    # -----------------------------------------------------------------
    stderr
    stderr "✅ Fresh template branch '${NEW_ORPHAN_BRANCH}' created – single commit \"${COMMIT_MSG}\"."
}


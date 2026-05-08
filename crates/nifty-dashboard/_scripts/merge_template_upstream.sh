#!/bin/bash

set -eo pipefail

SCRIPT_DIR="$(dirname ${BASH_SOURCE})"
PROJECT_DIR="$(realpath ${SCRIPT_DIR}/..)"

cd ${PROJECT_DIR}
source "${SCRIPT_DIR}/funcs.sh"

INTENDED_USER="EnigmaCurry"
RUN_ANYWAY="${RUN_ANYWAY:-false}"

if [[ "$(git config user.name)" != "${INTENDED_USER}" ]]; then
    if [[ "${RUN_ANYWAY,,}" != "true" ]]; then
        fault "Sorry, this script is only intended to be run by ${INTENDED_USER}. If you want to run it anyway, set the env var \`RUN_ANYWAY=true\`"
    fi
fi

DEV_APP_NAME="axum-dev"
TEMPLATE_URL="git@github.com:EnigmaCurry/rust-axum-template.git"
TEMPLATE_REPO="$(realpath ../rust-axum-template)"
PROJECT_SRC_DIR="${PROJECT_DIR}/${DEV_APP_NAME}/src"
TEMPLATE_SRC_DIR="${TEMPLATE_REPO}/template/PROJECT/src"
FRONTEND_SRC_DIR="${PROJECT_DIR}/frontend"
MIGRATIONS_DIR="${PROJECT_DIR}/${DEV_APP_NAME}/migrations"
TEST_DATA_DIR="${PROJECT_DIR}/${DEV_APP_NAME}/test_data"

if [[ ! -d "${TEMPLATE_REPO}" ]]; then
    fault "Could not find template repo"
fi
if [[ "$(git -C "${TEMPLATE_REPO}" remote get-url origin)" != "${TEMPLATE_URL}" ]]; then
    fault "template repo has wrong URL. Should be ${TEMPLATE_REPO}"
fi
if [[ ! -d "${PROJECT_SRC_DIR}" ]]; then
    fault "Could not find project src dir: ${PROJECT_SRC_DIR}"
fi
if [[ ! -d "${TEMPLATE_SRC_DIR}" ]]; then
    fault "Could not find template src dir: ${TEMPLATE_SRC_DIR}"
fi

check_git_clean "${PROJECT_DIR}"
check_git_clean "${TEMPLATE_REPO}"

echo
echo "It looks like you're all set to merge this:"
debug_var PROJECT_DIR
debug_var TEMPLATE_REPO
echo
confirm no "This will backport changes from PROJECT_DIR into TEMPLATE_REPO"
echo

set -x
rm -rf "${TEMPLATE_SRC_DIR}"
cp -a "${PROJECT_SRC_DIR}" "${TEMPLATE_SRC_DIR}"
rm -rf "${TEMPLATE_REPO}/_scripts"
cp -a "${PROJECT_DIR}/_scripts" "${TEMPLATE_REPO}/_scripts"
cp -a "${FRONTEND_SRC_DIR}" "${TEMPLATE_REPO}/template/"
cp -a "${MIGRATIONS_DIR}" "${TEMPLATE_REPO}/template/PROJECT"
cp -a "${TEST_DATA_DIR}" "${TEMPLATE_REPO}/template/PROJECT"
rm -rf "${TEMPLATE_REPO}/template/.github/workflows"/*

sed_escape() {
    # Escape / and & (the two characters that break a basic s/// command)
    printf '%s\n' "$1" | sed -e 's/[\/&]/\\&/g'
}

reverse_template_vars() {
    local input_file="$1"
    local output_file="$2"
    local escaped_dev_name=$(sed_escape "$DEV_APP_NAME")
    sed -e "s/${escaped_dev_name}/\${APP}/g" \
        -e 's/EnigmaCurry/\${GIT_USERNAME}/g' \
        -e 's/enigmacurry/\${GIT_USERNAME}/g' \
        "$input_file" > "$output_file"
}

reverse_template_vars_without_git_username() {
    local input_file="$1"
    local output_file="$2"
    local escaped_dev_name=$(sed_escape "$DEV_APP_NAME")
    sed -e "s/${escaped_dev_name}/\${APP}/g" \
        "$input_file" > "$output_file"
}


reverse_template_vars ${PROJECT_DIR}/Justfile ${TEMPLATE_REPO}/template/Justfile
reverse_template_vars ${PROJECT_DIR}/README.md ${TEMPLATE_REPO}/template/README.md
reverse_template_vars ${PROJECT_DIR}/DEVELOPMENT.md ${TEMPLATE_REPO}/template/DEVELOPMENT.md
reverse_template_vars ${PROJECT_DIR}/${DEV_APP_NAME}/Dockerfile ${TEMPLATE_REPO}/template/PROJECT/Dockerfile
reverse_template_vars ${PROJECT_DIR}/${DEV_APP_NAME}/Dockerfile.binary ${TEMPLATE_REPO}/template/PROJECT/Dockerfile.binary
reverse_template_vars ${PROJECT_DIR}/Cargo.toml ${TEMPLATE_REPO}/template/Cargo.toml
reverse_template_vars ${PROJECT_DIR}/${DEV_APP_NAME}/Cargo.toml ${TEMPLATE_REPO}/template/PROJECT/Cargo.toml
reverse_template_vars ${PROJECT_DIR}/.github/workflows/release.yml ${TEMPLATE_REPO}/template/.github/workflows/release.yml
reverse_template_vars ${PROJECT_DIR}/.github/workflows/rust.yml ${TEMPLATE_REPO}/template/.github/workflows/rust.yml
reverse_template_vars_without_git_username ${PROJECT_DIR}/Cargo.lock ${TEMPLATE_REPO}/template/Cargo.lock

echo
(cd ${TEMPLATE_REPO} && git add .)
echo "All files copied and staged for commit in ${TEMPLATE_REPO}"

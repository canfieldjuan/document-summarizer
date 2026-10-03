#!/usr/bin/env bash

set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPOSITORY_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
readonly TAURI_ROOT="${REPOSITORY_ROOT}/src-tauri"
readonly EXPECTED_FIXTURE_SHA256="12097e00b956e8f387e2cd43dd609a9cecc1ca1580c32cc3b87b60518307382b"

usage() {
  echo "Usage: $0 /absolute/path/to/dol-workplace-poster.pdf" >&2
}

fixture_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "A SHA-256 command is required (sha256sum or shasum)." >&2
    exit 69
  fi
}

if [[ $# -ne 1 ]]; then
  usage
  exit 64
fi

readonly GENERAL_FIXTURE="$1"
if [[ ! -f "${GENERAL_FIXTURE}" ]]; then
  echo "General acceptance fixture does not exist: ${GENERAL_FIXTURE}" >&2
  exit 66
fi

readonly ACTUAL_FIXTURE_SHA256="$(fixture_sha256 "${GENERAL_FIXTURE}")"
if [[ "${ACTUAL_FIXTURE_SHA256}" != "${EXPECTED_FIXTURE_SHA256}" ]]; then
  echo "General acceptance requires the documented public DOL fixture." >&2
  echo "Expected SHA-256: ${EXPECTED_FIXTURE_SHA256}" >&2
  echo "Actual SHA-256:   ${ACTUAL_FIXTURE_SHA256}" >&2
  exit 65
fi

if [[ -z "${DOC_SUM_QUALIFICATION_GGUF:-}" || ! -f "${DOC_SUM_QUALIFICATION_GGUF}" ]]; then
  echo "Set DOC_SUM_QUALIFICATION_GGUF to the pinned Qwen3.5-9B Q4_K_M file." >&2
  exit 66
fi

if [[ -n "${DOC_SUM_MODEL_SETTINGS_PATH:-}" ||
      -n "${DOC_SUM_QUALIFICATION_ANALYSIS_MODEL:-}" ||
      -n "${DOC_SUM_QUALIFICATION_VERIFICATION_MODEL:-}" ||
      -n "${DOC_SUM_QUALIFICATION_CONTEXT_TOKENS:-}" ]]; then
  echo "Default-profile acceptance requires other model-selection overrides to be unset." >&2
  exit 69
fi

cd "${REPOSITORY_ROOT}"
npm run build

# The test helper registers the supplied GGUF and uses the production default.
# The production runtime verifies model/runtime identities and owns the child.
export DOC_SUM_OFFICE_PDF="${GENERAL_FIXTURE}"
export DOC_SUM_OFFICE_TRACE_MODEL_RESPONSES=1

cd "${TAURI_ROOT}"

echo "PROFILE_RELEASE_ACCEPTANCE automatic routing"
cargo test --locked --lib \
  pipeline::profile_suggestion::tests::live_profile_suggestions_distinguish_dominant_purpose_counterexamples \
  -- --ignored --exact --nocapture

echo "PROFILE_RELEASE_ACCEPTANCE long Story"
cargo test --locked --lib \
  pipeline::summary::coherent::tests::live_story_profile_selects_then_generates_a_source_bound_synopsis \
  -- --ignored --exact --nocapture

echo "PROFILE_RELEASE_ACCEPTANCE long Contract"
cargo test --locked --lib \
  pipeline::summary::coherent::tests::live_contract_profile_selects_then_generates_a_source_bound_overview \
  -- --ignored --exact --nocapture

echo "PROFILE_RELEASE_ACCEPTANCE persisted long General"
cargo test --locked --test office_acceptance \
  office_pdf_live_ollama_summary_has_exact_durable_evidence \
  -- --ignored --exact --nocapture

echo "PROFILE_RELEASE_ACCEPTANCE mechanical checks complete; semantic review is still required."

#!/usr/bin/env bash

set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPOSITORY_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
readonly TAURI_ROOT="${REPOSITORY_ROOT}/src-tauri"
readonly EXPECTED_FIXTURE_SHA256="12097e00b956e8f387e2cd43dd609a9cecc1ca1580c32cc3b87b60518307382b"
readonly MODEL_NAME="${DOC_SUM_MODEL_NAME:-qwen3-30b-a3b:latest}"
readonly EXPECTED_MODEL_BASE_URL="http://127.0.0.1:11434/v1/"
readonly MODEL_BASE_URL="${DOC_SUM_MODEL_BASE_URL:-${EXPECTED_MODEL_BASE_URL}}"

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

if [[ ! -f "${REPOSITORY_ROOT}/dist/index.html" ]]; then
  echo "Frontend assets are missing. Run npm ci and npm run build first." >&2
  exit 69
fi

if [[ "${MODEL_BASE_URL}" != "${EXPECTED_MODEL_BASE_URL}" ]]; then
  echo "GPU acceptance requires ${EXPECTED_MODEL_BASE_URL}; got ${MODEL_BASE_URL}" >&2
  exit 69
fi

if [[ -n "${OLLAMA_HOST:-}" ]]; then
  echo "GPU acceptance requires OLLAMA_HOST to be unset so the CLI checks ${EXPECTED_MODEL_BASE_URL}." >&2
  exit 69
fi

if [[ -n "${DOC_SUM_MODEL_SETTINGS_PATH:-}" ||
      -n "${DOC_SUM_QUALIFICATION_ANALYSIS_MODEL:-}" ||
      -n "${DOC_SUM_QUALIFICATION_VERIFICATION_MODEL:-}" ||
      -n "${DOC_SUM_QUALIFICATION_CONTEXT_TOKENS:-}" ]]; then
  echo "GPU acceptance requires model-settings and qualification overrides to be unset." >&2
  exit 69
fi

if ! command -v ollama >/dev/null 2>&1; then
  echo "The Ollama CLI is required for the GPU residency check." >&2
  exit 69
fi

readonly OLLAMA_MODEL_ROW="$(ollama ps | awk -v model="${MODEL_NAME}" '$1 == model { print; exit }')"
if [[ -z "${OLLAMA_MODEL_ROW}" ]]; then
  echo "Model ${MODEL_NAME} is not loaded in Ollama." >&2
  exit 69
fi
if [[ "${OLLAMA_MODEL_ROW}" != *"100% GPU"* ]]; then
  echo "Model ${MODEL_NAME} is not running at 100% GPU: ${OLLAMA_MODEL_ROW}" >&2
  exit 69
fi

export DOC_SUM_MODEL_NAME="${MODEL_NAME}"
export DOC_SUM_MODEL_BASE_URL="${MODEL_BASE_URL}"
export DOC_SUM_OFFICE_PDF="${GENERAL_FIXTURE}"
export DOC_SUM_OFFICE_TRACE_MODEL_RESPONSES=1

cd "${TAURI_ROOT}"

echo "PROFILE_RELEASE_ACCEPTANCE automatic routing"
cargo test --lib \
  pipeline::profile_suggestion::tests::live_profile_suggestions_distinguish_dominant_purpose_counterexamples \
  -- --ignored --exact --nocapture

echo "PROFILE_RELEASE_ACCEPTANCE long Story"
cargo test --lib \
  pipeline::summary::coherent::tests::live_story_profile_selects_then_generates_a_source_bound_synopsis \
  -- --ignored --exact --nocapture

echo "PROFILE_RELEASE_ACCEPTANCE long Contract"
cargo test --lib \
  pipeline::summary::coherent::tests::live_contract_profile_selects_then_generates_a_source_bound_overview \
  -- --ignored --exact --nocapture

echo "PROFILE_RELEASE_ACCEPTANCE persisted long General"
cargo test --test office_acceptance \
  office_pdf_live_ollama_summary_has_exact_durable_evidence \
  -- --ignored --exact --nocapture

echo "PROFILE_RELEASE_ACCEPTANCE mechanical checks complete; semantic review is still required."

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

type PipelineState =
  | "Received"
  | "Ingesting"
  | "Ingested"
  | "Parsing"
  | "Parsed"
  | "VisualAnalysisRequired"
  | "VisualAnalyzing"
  | "VisualAnalyzed"
  | "Normalizing"
  | "Normalized"
  | "Structuring"
  | "Structured"
  | "Chunking"
  | "Chunked"
  | "Analyzing"
  | "Analyzed"
  | "Synthesizing"
  | "Synthesized"
  | "Verifying"
  | "Verified"
  | "Complete"
  | "CompleteWithWarnings"
  | "Failed"
  | "Cancelling"
  | "Cancelled";

interface PipelineWarning {
  code: string;
  message: string;
}

interface PipelineFailure {
  code: string;
  message: string;
  recoverable: boolean;
}

interface RuntimeStatus {
  providerName: string;
  ready: boolean;
  runtimeId: string | null;
  modelId: string | null;
  code: string | null;
  message: string;
  recoverable: boolean;
}

interface RunHistoryItem {
  runId: string;
  documentId: string;
  originalFilename: string;
  byteSize: number;
  state: PipelineState;
  stateVersion: number;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
  warnings: PipelineWarning[];
  failure: PipelineFailure | null;
  hasSummary: boolean;
}

interface SummaryArtifact {
  text: string;
  warnings: PipelineWarning[];
  createdAt: string;
  claims: CitedClaim[];
}

interface CitedClaim {
  claimId: string;
  text: string;
  citations: Citation[];
}

interface Citation {
  evidenceId: string;
  label: string;
  pageStart: number;
  pageEnd: number;
  exactQuote: string;
}

interface PersistedSummary {
  run: RunHistoryItem;
  summary: SummaryArtifact;
}

interface CompletedSummary {
  runId: string;
  originalFilename: string;
  byteSize: number;
  summary: SummaryArtifact;
}

interface CommandError {
  code: string;
  message: string;
}

const selectButton = element<HTMLButtonElement>("#select-btn");
const actionHint = element<HTMLParagraphElement>("#action-hint");
const runtimeDot = element<HTMLSpanElement>("#runtime-dot");
const runtimeTitle = element<HTMLParagraphElement>("#runtime-title");
const runtimeDetail = element<HTMLParagraphElement>("#runtime-detail");
const runtimeRetry = element<HTMLButtonElement>("#runtime-retry");
const historyRefresh = element<HTMLButtonElement>("#history-refresh");
const historyStatus = element<HTMLParagraphElement>("#history-status");
const historyList = element<HTMLOListElement>("#history-list");
const emptyView = element<HTMLDivElement>("#empty-view");
const processingView = element<HTMLDivElement>("#processing-view");
const summaryView = element<HTMLElement>("#summary-view");
const failureView = element<HTMLDivElement>("#failure-view");
const processingFilename = element<HTMLHeadingElement>("#processing-filename");
const summaryFilename = element<HTMLHeadingElement>("#summary-filename");
const summaryMeta = element<HTMLParagraphElement>("#summary-meta");
const summaryText = element<HTMLPreElement>("#summary-text");
const summaryClaims = element<HTMLDivElement>("#summary-claims");
const evidencePanel = element<HTMLElement>("#evidence-panel");
const evidenceLabel = element<HTMLParagraphElement>("#evidence-label");
const evidenceQuote = element<HTMLQuoteElement>("#evidence-quote");
const warningSection = element<HTMLElement>("#warning-section");
const warningList = element<HTMLUListElement>("#warning-list");
const failureTitle = element<HTMLHeadingElement>("#failure-title");
const failureMessage = element<HTMLParagraphElement>("#failure-message");
const failureCode = element<HTMLParagraphElement>("#failure-code");

let runtimeReady = false;
let processing = false;
let activeRunId: string | null = null;
let recentRuns: RunHistoryItem[] = [];

function element<T extends HTMLElement>(selector: string): T {
  const match = document.querySelector<T>(selector);
  if (!match) {
    throw new Error(`Required application element is missing: ${selector}`);
  }
  return match;
}

function showStage(view: "empty" | "processing" | "summary" | "failure"): void {
  emptyView.hidden = view !== "empty";
  processingView.hidden = view !== "processing";
  summaryView.hidden = view !== "summary";
  failureView.hidden = view !== "failure";
}

function syncPrimaryAction(): void {
  selectButton.disabled = !runtimeReady || processing;
  const label = selectButton.querySelector<HTMLSpanElement>("span");
  if (!label) return;

  if (processing) {
    label.textContent = "Processing…";
    actionHint.textContent = "The source is being processed locally.";
  } else if (runtimeReady) {
    label.textContent = "Choose a PDF";
    actionHint.textContent = "Native-text PDFs are supported in this release.";
  } else {
    label.textContent = "Ollama unavailable";
    actionHint.textContent = "Start Ollama and check the selected model, then try again.";
  }
}

async function refreshRuntimeStatus(): Promise<void> {
  runtimeReady = false;
  runtimeDot.className = "runtime-dot is-checking";
  runtimeTitle.textContent = "Checking Ollama";
  runtimeDetail.textContent = "Looking for the local model…";
  runtimeRetry.hidden = true;
  syncPrimaryAction();

  try {
    const status = await invoke<RuntimeStatus>("get_runtime_status");
    runtimeReady = status.ready;
    runtimeDot.className = status.ready
      ? "runtime-dot is-ready"
      : "runtime-dot is-unavailable";
    runtimeTitle.textContent = status.ready
      ? `${status.providerName} ready`
      : `${status.providerName} unavailable`;
    runtimeDetail.textContent = status.ready && status.modelId
      ? status.modelId
      : status.message;
    runtimeRetry.hidden = status.ready;
  } catch (error) {
    const commandError = normalizeCommandError(error);
    runtimeDot.className = "runtime-dot is-unavailable";
    runtimeTitle.textContent = "Runtime check failed";
    runtimeDetail.textContent = commandError.message;
    runtimeRetry.hidden = false;
  } finally {
    syncPrimaryAction();
  }
}

async function refreshHistory(): Promise<void> {
  historyRefresh.disabled = true;
  historyStatus.hidden = false;
  historyStatus.textContent = "Loading recent documents…";

  try {
    recentRuns = await invoke<RunHistoryItem[]>("list_recent_runs");
    renderHistory();
  } catch (error) {
    recentRuns = [];
    historyList.replaceChildren();
    historyStatus.textContent = normalizeCommandError(error).message;
  } finally {
    historyRefresh.disabled = false;
  }
}

function renderHistory(): void {
  historyList.replaceChildren();
  if (recentRuns.length === 0) {
    historyStatus.hidden = false;
    historyStatus.textContent = "No local runs yet. Your completed summaries will stay here.";
    return;
  }

  historyStatus.hidden = true;
  recentRuns.forEach((run, index) => {
    const item = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    button.className = "history-item-button";
    button.classList.toggle("is-active", run.runId === activeRunId);
    button.setAttribute("aria-label", `Open ${run.originalFilename}, ${stateLabel(run.state)}`);
    button.addEventListener("click", () => void openHistoryRun(run));

    const ordinal = document.createElement("span");
    ordinal.className = "run-index";
    ordinal.textContent = String(index + 1).padStart(2, "0");

    const copy = document.createElement("span");
    const name = document.createElement("span");
    name.className = "run-name";
    name.textContent = run.originalFilename;

    const detail = document.createElement("span");
    detail.className = "run-detail";
    detail.textContent = `${formatBytes(run.byteSize)} · ${formatDate(run.completedAt ?? run.updatedAt)}`;

    const state = document.createElement("span");
    state.className = "run-state";
    state.classList.toggle("is-failed", run.state === "Failed" || run.state === "Cancelled");
    state.textContent = stateLabel(run.state);

    copy.append(name, detail, state);
    button.append(ordinal, copy);
    item.append(button);
    historyList.append(item);
  });
}

async function openHistoryRun(run: RunHistoryItem): Promise<void> {
  activeRunId = run.runId;
  renderHistory();

  if (run.failure) {
    showFailure(run.originalFilename, run.failure.message, run.failure.code);
    return;
  }

  if (!run.hasSummary) {
    showFailure(
      run.originalFilename,
      `This run stopped in ${stateLabel(run.state).toLowerCase()} and has no completed summary.`,
      "SUMMARY_NOT_AVAILABLE",
    );
    return;
  }

  showStage("processing");
  processingFilename.textContent = `Opening ${run.originalFilename}`;
  try {
    const persisted = await invoke<PersistedSummary>("get_persisted_summary", {
      runId: run.runId,
    });
    renderSummary(
      persisted.run.originalFilename,
      persisted.run.byteSize,
      persisted.summary,
    );
  } catch (error) {
    const commandError = normalizeCommandError(error);
    showFailure(run.originalFilename, commandError.message, commandError.code);
  }
}

async function selectAndSummarize(): Promise<void> {
  if (!runtimeReady || processing) {
    return;
  }

  try {
    const selected = await open({
      multiple: false,
      filters: [{
        name: "PDF",
        extensions: ["pdf"],
      }],
    });
    if (selected === null) {
      return;
    }

    const filename = displayFilename(selected);
    processing = true;
    activeRunId = null;
    processingFilename.textContent = filename;
    showStage("processing");
    syncPrimaryAction();

    const completed = await invoke<CompletedSummary>("summarize_document", {
      filePath: selected,
    });
    activeRunId = completed.runId;
    renderSummary(
      completed.originalFilename,
      completed.byteSize,
      completed.summary,
    );
  } catch (error) {
    const commandError = normalizeCommandError(error);
    showFailure("Processing stopped safely", commandError.message, commandError.code);
  } finally {
    processing = false;
    syncPrimaryAction();
    await refreshHistory();
  }
}

function renderSummary(filename: string, byteSize: number, summary: SummaryArtifact): void {
  summaryFilename.textContent = filename;
  const citedClaimCount = summary.claims.length;
  summaryMeta.textContent = citedClaimCount > 0
    ? `${formatBytes(byteSize)} · ${formatDate(summary.createdAt)} · ${citedClaimCount} cited ${citedClaimCount === 1 ? "claim" : "claims"}`
    : `${formatBytes(byteSize)} · ${formatDate(summary.createdAt)}`;
  renderClaims(summary);
  renderWarnings(summary.warnings);
  showStage("summary");
}

function renderClaims(summary: SummaryArtifact): void {
  summaryClaims.replaceChildren();
  evidencePanel.hidden = true;
  evidenceLabel.textContent = "";
  evidenceQuote.textContent = "";

  if (summary.claims.length === 0) {
    summaryClaims.hidden = true;
    summaryText.hidden = false;
    summaryText.textContent = summary.text;
    return;
  }

  summaryText.textContent = "";
  summaryText.hidden = true;
  summaryClaims.hidden = false;
  summary.claims.forEach((claim, claimIndex) => {
    const item = document.createElement("section");
    item.className = "summary-claim";

    const ordinal = document.createElement("p");
    ordinal.className = "claim-ordinal";
    ordinal.textContent = `Claim ${String(claimIndex + 1).padStart(2, "0")}`;

    const text = document.createElement("p");
    text.className = "claim-text";
    text.textContent = claim.text;

    const actions = document.createElement("div");
    actions.className = "citation-actions";
    actions.setAttribute("aria-label", `Evidence for claim ${claimIndex + 1}`);
    for (const citation of claim.citations) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "citation-button";
      button.textContent = citation.label;
      button.setAttribute("aria-pressed", "false");
      button.setAttribute(
        "aria-label",
        `Show exact source excerpt from ${citation.label} for claim ${claimIndex + 1}`,
      );
      button.addEventListener("click", () => showEvidence(citation, button));
      actions.append(button);
    }

    item.append(ordinal, text, actions);
    summaryClaims.append(item);
  });
}

function showEvidence(citation: Citation, selected: HTMLButtonElement): void {
  for (const button of summaryClaims.querySelectorAll<HTMLButtonElement>(".citation-button")) {
    const isSelected = button === selected;
    button.classList.toggle("is-active", isSelected);
    button.setAttribute("aria-pressed", String(isSelected));
  }
  evidenceLabel.textContent = citation.label;
  evidenceQuote.textContent = citation.exactQuote;
  evidencePanel.hidden = false;
}

function renderWarnings(warnings: PipelineWarning[]): void {
  warningList.replaceChildren();
  warningSection.hidden = warnings.length === 0;
  for (const warning of warnings) {
    const item = document.createElement("li");
    item.textContent = warning.message;
    warningList.append(item);
  }
}

function showFailure(title: string, message: string, code: string): void {
  failureTitle.textContent = title;
  failureMessage.textContent = message;
  failureCode.textContent = code;
  showStage("failure");
}

function normalizeCommandError(error: unknown): CommandError {
  if (typeof error === "object" && error !== null) {
    const value = error as Record<string, unknown>;
    if (typeof value.code === "string" && typeof value.message === "string") {
      return { code: value.code, message: value.message };
    }
  }
  if (error instanceof Error) {
    return { code: "APPLICATION_ERROR", message: error.message };
  }
  return { code: "APPLICATION_ERROR", message: String(error) };
}

function displayFilename(path: string): string {
  const segments = path.split(/[\\/]/);
  return segments[segments.length - 1] || "Selected PDF";
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatDate(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "Date unavailable";
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
}

function stateLabel(state: PipelineState): string {
  const labels: Record<PipelineState, string> = {
    Received: "Received",
    Ingesting: "Ingesting",
    Ingested: "Ingested",
    Parsing: "Parsing",
    Parsed: "Parsed",
    VisualAnalysisRequired: "Visual review required",
    VisualAnalyzing: "Visual processing",
    VisualAnalyzed: "Visual processing complete",
    Normalizing: "Normalizing",
    Normalized: "Normalized",
    Structuring: "Structuring",
    Structured: "Structured",
    Chunking: "Chunking",
    Chunked: "Chunked",
    Analyzing: "Analyzing",
    Analyzed: "Analyzed",
    Synthesizing: "Synthesizing",
    Synthesized: "Synthesized",
    Verifying: "Verifying",
    Verified: "Verified",
    Complete: "Complete",
    CompleteWithWarnings: "Complete with notes",
    Failed: "Failed safely",
    Cancelling: "Cancelling",
    Cancelled: "Cancelled",
  };
  return labels[state];
}

async function initialize(): Promise<void> {
  selectButton.addEventListener("click", () => void selectAndSummarize());
  runtimeRetry.addEventListener("click", () => void refreshRuntimeStatus());
  historyRefresh.addEventListener("click", () => void refreshHistory());
  await Promise.all([refreshRuntimeStatus(), refreshHistory()]);
}

void initialize();

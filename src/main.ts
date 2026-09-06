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

interface ModelPreset {
  presetId: string;
  label: string;
  mode: "full" | "hybrid";
  analysisModel: string;
  analysisDigest: string;
  analysisContextTokens: number;
  verificationModel: string;
  verificationDigest: string;
  verificationContextTokens: number;
}

interface ModelCatalog {
  selectedPresetId: string;
  selectedPresetAvailable: boolean;
  installedModels: ModelOption[];
  presets: ModelPreset[];
}

interface ModelOption {
  name: string;
  digest: string;
  sizeBytes: number;
  architecture: string | null;
  tokenizerFamily: "qwen3" | "qwen35" | null;
  parameterSize: string | null;
  quantizationLevel: string | null;
  maximumContextTokens: number | null;
  disabledReason: string | null;
  profileId: string | null;
  qualifiedContextTokens: number | null;
  supportsAnalysis: boolean;
  supportsVerification: boolean;
  supportsFull: boolean;
}

type ConnectEntitlementState =
  | "active"
  | "authority_unavailable"
  | "missing"
  | "invalid"
  | "not_yet_valid"
  | "expired"
  | "feature_missing";

interface ConnectEntitlementStatus {
  state: ConnectEntitlementState;
  active: boolean;
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
  retryOfRunId: string | null;
  retryRunId: string | null;
  canRetry: boolean;
  continuationCheckpoint: PipelineState | null;
  canContinue: boolean;
  continuationRequiresRuntime: boolean;
  cancellationRequested: boolean;
  backgroundActive: boolean;
  canCancel: boolean;
}

interface BackgroundRunAccepted {
  runId: string;
  documentId: string;
  originalFilename: string;
  byteSize: number;
  state: PipelineState;
  stateVersion: number;
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
const modelPreset = element<HTMLSelectElement>("#model-preset");
const connectMark = element<HTMLSpanElement>("#connect-mark");
const connectTitle = element<HTMLParagraphElement>("#connect-title");
const connectDetail = element<HTMLParagraphElement>("#connect-detail");
const connectActivate = element<HTMLButtonElement>("#connect-activate");
const historyRefresh = element<HTMLButtonElement>("#history-refresh");
const recoveryNotice = element<HTMLElement>("#recovery-notice");
const recoveryNoticeTitle = element<HTMLParagraphElement>("#recovery-notice-title");
const recoveryNoticeDetail = element<HTMLParagraphElement>("#recovery-notice-detail");
const historyStatus = element<HTMLParagraphElement>("#history-status");
const historyList = element<HTMLOListElement>("#history-list");
const emptyView = element<HTMLDivElement>("#empty-view");
const processingView = element<HTMLDivElement>("#processing-view");
const summaryView = element<HTMLElement>("#summary-view");
const failureView = element<HTMLDivElement>("#failure-view");
const processingFilename = element<HTMLHeadingElement>("#processing-filename");
const processingStatus = element<HTMLParagraphElement>("#processing-status");
const cancelButton = element<HTMLButtonElement>("#cancel-btn");
const cancelHint = element<HTMLParagraphElement>("#cancel-hint");
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
const failureKicker = element<HTMLParagraphElement>("#failure-kicker");
const failureMessage = element<HTMLParagraphElement>("#failure-message");
const failureCode = element<HTMLParagraphElement>("#failure-code");
const continueButton = element<HTMLButtonElement>("#continue-btn");
const continueHint = element<HTMLParagraphElement>("#continue-hint");
const retryButton = element<HTMLButtonElement>("#retry-btn");
const retryHint = element<HTMLParagraphElement>("#retry-hint");

let runtimeReady = false;
let selectedModelLabel: string | null = null;
let modelSelectionAvailable = false;
let modelSelectionInFlight = false;
let connectInstalling = false;
let connectStatusRefreshInFlight = false;
let processing = false;
let processingRun: RunHistoryItem | null = null;
let processingRunId: string | null = null;
let activeRunId: string | null = null;
let recentRuns: RunHistoryItem[] = [];
let retrySourceRun: RunHistoryItem | null = null;
let continuationRun: RunHistoryItem | null = null;

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
  modelPreset.disabled = !modelSelectionAvailable || modelSelectionInFlight || processing;
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

  if (retrySourceRun) {
    retryButton.disabled = !runtimeReady || processing;
    retryHint.textContent = processing
      ? "Creating a separate retry attempt…"
      : runtimeReady
        ? "A new attempt will reuse the durable document identity. This failed record stays unchanged."
        : "Start Ollama before retrying this document.";
  }

  if (continuationRun) {
    const runtimeAvailable = !continuationRun.continuationRequiresRuntime || runtimeReady;
    continueButton.disabled = !runtimeAvailable || processing;
    continueHint.textContent = processing
      ? "Continuing from the durable checkpoint…"
      : runtimeAvailable
        ? `Continue this run from ${stateLabel(continuationRun.state).toLowerCase()} without repeating completed stages.`
        : "Start Ollama before continuing this checkpoint.";
  }

  cancelButton.hidden = !processing;
  cancelButton.disabled = !processingRun?.canCancel;
  cancelHint.hidden = !processing;
  if (processing) {
    cancelHint.textContent = processingRun?.cancellationRequested
      ? "Finishing the current safe work unit before cancellation completes."
      : processingRun?.canCancel
        ? "Cancellation preserves completed checkpoints and discards unfinished output."
        : "Waiting for the background worker to reach a cancellable checkpoint.";
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
    runtimeDetail.textContent = status.ready && selectedModelLabel
      ? selectedModelLabel
      : status.ready && status.modelId
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

async function refreshModelCatalog(): Promise<void> {
  modelSelectionAvailable = false;
  modelPreset.disabled = true;
  try {
    const catalog = await invoke<ModelCatalog>("get_model_catalog");
    modelPreset.replaceChildren();
    const qualified = document.createElement("optgroup");
    qualified.label = "Qualified presets";
    for (const preset of catalog.presets) {
      const installed = catalog.installedModels.find(
        (model) => model.digest === preset.analysisDigest,
      );
      const option = document.createElement("option");
      option.value = preset.presetId;
      const safeContext = preset.mode === "hybrid"
        ? preset.analysisContextTokens
        : preset.verificationContextTokens;
      const maximumContext = installed?.maximumContextTokens;
      option.textContent = [
        preset.label,
        installed ? formatModelSize(installed.sizeBytes) : null,
        `${safeContext.toLocaleString()} safe ctx`,
        maximumContext ? `${maximumContext.toLocaleString()} max` : null,
      ].filter(Boolean).join(" · ");
      qualified.append(option);
    }
    if (qualified.children.length > 0) modelPreset.append(qualified);
    const unavailable = document.createElement("optgroup");
    unavailable.label = "Installed but unavailable";
    for (const installed of catalog.installedModels.filter(
      (model) => !model.profileId || model.disabledReason,
    )) {
      const option = document.createElement("option");
      option.disabled = true;
      option.textContent = [
        installed.name,
        formatModelSize(installed.sizeBytes),
        installed.maximumContextTokens
          ? `${installed.maximumContextTokens.toLocaleString()} max ctx`
          : null,
        installed.disabledReason ?? "Not qualified on the live corpus",
      ].filter(Boolean).join(" · ");
      unavailable.append(option);
    }
    if (unavailable.children.length > 0) modelPreset.append(unavailable);
    modelPreset.value = catalog.selectedPresetId;
    const selectedPreset = catalog.presets.find(
      (preset) => preset.presetId === catalog.selectedPresetId,
    );
    const selectedInstalled = selectedPreset
      ? catalog.installedModels.find((model) => model.digest === selectedPreset.analysisDigest)
      : null;
    selectedModelLabel = selectedPreset
      ? [
          selectedPreset.label,
          selectedInstalled ? formatModelSize(selectedInstalled.sizeBytes) : null,
          `${selectedPreset.analysisContextTokens.toLocaleString()} ctx`,
        ].filter(Boolean).join(" · ")
      : null;
    modelSelectionAvailable = catalog.presets.length > 0;
    modelPreset.disabled = !modelSelectionAvailable || modelSelectionInFlight || processing;
    if (!catalog.selectedPresetAvailable && catalog.presets.length > 0) {
      const recovery = document.createElement("option");
      recovery.value = "";
      recovery.textContent = "Select an available qualified model";
      recovery.selected = true;
      modelPreset.prepend(recovery);
      modelPreset.disabled = modelSelectionInFlight || processing;
    }
  } catch (error) {
    const commandError = normalizeCommandError(error);
    selectedModelLabel = null;
    modelSelectionAvailable = false;
    modelPreset.replaceChildren();
    const option = document.createElement("option");
    option.textContent = commandError.message;
    modelPreset.append(option);
  }
}

function formatModelSize(bytes: number): string {
  return `${(bytes / (1024 ** 3)).toFixed(1)} GiB`;
}

async function selectModelPreset(): Promise<void> {
  const presetId = modelPreset.value;
  if (!presetId || processing) return;
  modelSelectionInFlight = true;
  runtimeReady = false;
  syncPrimaryAction();
  try {
    await invoke<ModelCatalog>("select_model_preset", { presetId });
    await refreshModelCatalog();
  } catch (error) {
    runtimeDetail.textContent = normalizeCommandError(error).message;
  } finally {
    await refreshRuntimeStatus();
    modelSelectionInFlight = false;
    syncPrimaryAction();
  }
}

function renderConnectStatus(status: ConnectEntitlementStatus): void {
  connectMark.className = status.active
    ? "connect-mark is-active"
    : "connect-mark is-unavailable";
  connectActivate.disabled = connectInstalling;
  connectActivate.hidden = status.state === "authority_unavailable";
  connectActivate.textContent = status.active ? "Replace license" : "Activate";

  const content: Record<ConnectEntitlementState, [string, string]> = {
    active: [
      "Connect active",
      "Other installed apps can use compatible document capabilities.",
    ],
    authority_unavailable: [
      "Connect unavailable in this build",
      "Install an official Connect-enabled build to activate a license.",
    ],
    missing: [
      "Connect not activated",
      "Install your Connect license to enable app-to-app capabilities.",
    ],
    invalid: [
      "Connect license invalid",
      "Choose a valid signed Connect license to restore capabilities.",
    ],
    not_yet_valid: [
      "Connect license not active yet",
      "This license cannot be used before its signed start time.",
    ],
    expired: [
      "Connect license expired",
      "Install a current signed license to restore capabilities.",
    ],
    feature_missing: [
      "Connect access not included",
      "This license does not include app-to-app capability exchange.",
    ],
  };
  [connectTitle.textContent, connectDetail.textContent] = content[status.state];
}

async function refreshConnectStatus(): Promise<void> {
  if (connectInstalling || connectStatusRefreshInFlight) return;
  connectStatusRefreshInFlight = true;
  connectMark.className = "connect-mark is-checking";
  connectTitle.textContent = "Checking Connect";
  connectDetail.textContent = "Looking for your license…";
  connectActivate.hidden = true;
  try {
    renderConnectStatus(
      await invoke<ConnectEntitlementStatus>("get_connect_entitlement_status"),
    );
  } catch (error) {
    const commandError = normalizeCommandError(error);
    connectMark.className = "connect-mark is-unavailable";
    connectTitle.textContent = "Connect status unavailable";
    connectDetail.textContent = commandError.message;
  } finally {
    connectStatusRefreshInFlight = false;
  }
}

async function selectAndInstallConnectEntitlement(): Promise<void> {
  if (connectInstalling) return;
  let selected: string | null;
  try {
    selected = await open({
      multiple: false,
      filters: [{
        name: "Connect license",
        extensions: ["json"],
      }],
    });
  } catch (error) {
    const commandError = normalizeCommandError(error);
    connectTitle.textContent = "License selection failed";
    connectDetail.textContent = commandError.message;
    return;
  }
  if (selected === null) return;

  connectInstalling = true;
  connectActivate.disabled = true;
  connectMark.className = "connect-mark is-checking";
  connectTitle.textContent = "Activating Connect";
  connectDetail.textContent = "Verifying and installing your signed license…";
  try {
    renderConnectStatus(await invoke<ConnectEntitlementStatus>(
      "install_connect_entitlement",
      { sourcePath: selected },
    ));
  } catch (error) {
    const commandError = normalizeCommandError(error);
    try {
      const current = await invoke<ConnectEntitlementStatus>("get_connect_entitlement_status");
      renderConnectStatus(current);
      connectTitle.textContent = current.active
        ? "Connect active — replacement failed"
        : "Activation failed";
      connectDetail.textContent = commandError.message;
    } catch {
      connectMark.className = "connect-mark is-unavailable";
      connectTitle.textContent = "Activation failed";
      connectDetail.textContent = commandError.message;
      connectActivate.hidden = false;
    }
  } finally {
    connectInstalling = false;
    connectActivate.disabled = false;
  }
}

async function refreshHistory(): Promise<void> {
  historyRefresh.disabled = true;
  historyStatus.hidden = false;
  historyStatus.textContent = "Loading recent documents…";

  try {
    recentRuns = await invoke<RunHistoryItem[]>("list_recent_runs");
    renderHistory();
    renderRecoveryNotice();
  } catch (error) {
    recentRuns = [];
    historyList.replaceChildren();
    recoveryNotice.hidden = true;
    historyStatus.textContent = normalizeCommandError(error).message;
  } finally {
    historyRefresh.disabled = false;
  }
}

function renderRecoveryNotice(): void {
  const interrupted = recentRuns.filter((run) => run.failure?.code === "PROCESS_INTERRUPTED");
  if (interrupted.length === 0) {
    recoveryNotice.hidden = true;
    return;
  }

  const retryable = interrupted.filter((run) => run.canRetry).length;
  recoveryNoticeTitle.textContent = interrupted.length === 1
    ? "Interrupted work recovered safely"
    : `${interrupted.length} interrupted runs recovered safely`;
  recoveryNoticeDetail.textContent = retryable > 0
    ? "Open the interrupted item below to create a separate retry attempt."
    : "The interrupted attempts remain in local history; no work was replayed automatically.";
  recoveryNotice.hidden = false;
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
    state.textContent = historyStateLabel(run);

    copy.append(name, detail, state);
    button.append(ordinal, copy);
    item.append(button);
    historyList.append(item);
  });
}
async function openHistoryRun(run: RunHistoryItem): Promise<void> {
  activeRunId = run.runId;
  renderHistory();

  if (isMonitoredBackgroundRun(run)) {
    if (processingRunId === run.runId) {
      processingFilename.textContent = run.originalFilename;
      processingStatus.textContent = processingStatusText(run);
      showStage("processing");
      syncPrimaryAction();
      return;
    }
    beginProcessing(run, run.originalFilename, processingStatusText(run));
    await monitorBackgroundRun(run.runId, run.originalFilename);
    return;
  }

  if (run.state === "Cancelled") {
    showFailure(
      run.originalFilename,
      "Processing was cancelled. Completed checkpoints remain in the local record.",
      "PROCESS_CANCELLED",
    );
    return;
  }

  if (run.failure) {
    showFailure(run.originalFilename, run.failure.message, run.failure.code, run);
    return;
  }

  if (!run.hasSummary) {
    showFailure(
      run.originalFilename,
      `This run stopped in ${stateLabel(run.state).toLowerCase()} and has no completed summary.`,
      "SUMMARY_NOT_AVAILABLE",
      run,
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

function beginProcessing(
  run: RunHistoryItem | null,
  title: string,
  status: string,
): void {
  processing = true;
  processingRun = run;
  processingRunId = run?.runId ?? null;
  activeRunId = run?.runId ?? null;
  processingFilename.textContent = title;
  processingStatus.textContent = status;
  showStage("processing");
  syncPrimaryAction();
}

function stopProcessing(): void {
  processing = false;
  processingRun = null;
  processingRunId = null;
  syncPrimaryAction();
}

async function monitorBackgroundRun(runId: string, fallbackFilename: string): Promise<void> {
  processingRunId = runId;
  activeRunId = runId;

  while (processing && processingRunId === runId) {
    let run: RunHistoryItem;
    try {
      run = await invoke<RunHistoryItem>("get_run_status", { runId });
    } catch (error) {
      stopProcessing();
      const commandError = normalizeCommandError(error);
      showFailure(fallbackFilename, commandError.message, commandError.code);
      await refreshHistory();
      return;
    }

    processingRun = run;
    upsertRecentRun(run);
    if (activeRunId === runId) {
      processingFilename.textContent = run.originalFilename;
      processingStatus.textContent = processingStatusText(run);
      showStage("processing");
    }
    syncPrimaryAction();

    if (isTerminalState(run.state)) {
      const shouldRender = activeRunId === runId;
      stopProcessing();
      await refreshHistory();
      if (!shouldRender) {
        return;
      }
      if (run.state === "Complete" || run.state === "CompleteWithWarnings") {
        try {
          const persisted = await invoke<PersistedSummary>("get_persisted_summary", { runId });
          renderSummary(
            persisted.run.originalFilename,
            persisted.run.byteSize,
            persisted.summary,
          );
        } catch (error) {
          const commandError = normalizeCommandError(error);
          showFailure(run.originalFilename, commandError.message, commandError.code);
        }
      } else if (run.state === "Cancelled") {
        showFailure(
          run.originalFilename,
          "Processing was cancelled. Completed checkpoints remain in the local record.",
          "PROCESS_CANCELLED",
        );
      } else {
        const failure = run.failure ?? {
          code: "BACKGROUND_PROCESSING_FAILED",
          message: "Background processing stopped safely.",
          recoverable: true,
        };
        showFailure(run.originalFilename, failure.message, failure.code, run);
      }
      return;
    }

    if (!run.backgroundActive) {
      const shouldRender = activeRunId === runId;
      stopProcessing();
      await refreshHistory();
      if (shouldRender) {
        const latest = recentRuns.find((item) => item.runId === runId);
        if (latest) {
          await openHistoryRun(latest);
        } else {
          showFailure(
            fallbackFilename,
            "The background worker stopped before a terminal result was persisted.",
            "BACKGROUND_JOB_NOT_RUNNING",
          );
        }
      }
      return;
    }

    await delay(500);
  }
}

async function cancelSelectedRun(): Promise<void> {
  const run = processingRun;
  if (!run?.canCancel) {
    return;
  }
  cancelButton.disabled = true;
  processingStatus.textContent = "Requesting cancellation…";
  try {
    processingRun = await invoke<RunHistoryItem>("cancel_document", {
      runId: run.runId,
      expectedStateVersion: run.stateVersion,
    });
    processingStatus.textContent = processingStatusText(processingRun);
  } catch (error) {
    const commandError = normalizeCommandError(error);
    processingStatus.textContent = commandError.code === "BACKGROUND_STALE_STATE"
      ? "The pipeline advanced while cancellation was requested; refreshing its current stage…"
      : commandError.message;
  } finally {
    syncPrimaryAction();
  }
}

function upsertRecentRun(run: RunHistoryItem): void {
  const existing = recentRuns.findIndex((item) => item.runId === run.runId);
  if (existing >= 0) {
    recentRuns[existing] = run;
  } else {
    recentRuns.unshift(run);
  }
  recentRuns.sort((left, right) =>
    right.updatedAt.localeCompare(left.updatedAt) || right.runId.localeCompare(left.runId));
  renderHistory();
}

function processingStatusText(run: RunHistoryItem): string {
  if (run.cancellationRequested || run.state === "Cancelling") {
    return "Cancellation requested; finishing the current safe work unit…";
  }
  return `${stateLabel(run.state)} in the background…`;
}

function isTerminalState(state: PipelineState): boolean {
  return state === "Complete"
    || state === "CompleteWithWarnings"
    || state === "Failed"
    || state === "Cancelled";
}

function isMonitoredBackgroundRun(run: RunHistoryItem): boolean {
  return run.backgroundActive && !isTerminalState(run.state);
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
}

async function selectAndSummarize(): Promise<void> {
  if (!runtimeReady || processing) {
    return;
  }

  let selected: string | null;
  try {
    selected = await open({
      multiple: false,
      filters: [{
        name: "PDF",
        extensions: ["pdf"],
      }],
    });
  } catch (error) {
    const commandError = normalizeCommandError(error);
    showFailure("File selection failed", commandError.message, commandError.code);
    return;
  }
  if (selected === null) {
    return;
  }

  const filename = displayFilename(selected);
  beginProcessing(null, filename, "Preparing the durable pipeline run…");
  try {
    const accepted = await invoke<BackgroundRunAccepted>("summarize_document", {
      filePath: selected,
    });
    await monitorBackgroundRun(accepted.runId, accepted.originalFilename);
  } catch (error) {
    stopProcessing();
    const commandError = normalizeCommandError(error);
    showFailure("Processing stopped safely", commandError.message, commandError.code);
    await refreshHistory();
  }
}

async function retrySelectedRun(): Promise<void> {
  const source = retrySourceRun;
  if (!source || !source.canRetry || !runtimeReady || processing) {
    return;
  }

  beginProcessing(source, `Retrying ${source.originalFilename}`, "Creating a separate retry run…");

  let accepted: BackgroundRunAccepted | null = null;
  let commandError: CommandError | null = null;
  try {
    accepted = await invoke<BackgroundRunAccepted>("retry_document", {
      runId: source.runId,
      expectedStateVersion: source.stateVersion,
    });
  } catch (error) {
    commandError = normalizeCommandError(error);
  }

  if (accepted) {
    await monitorBackgroundRun(accepted.runId, accepted.originalFilename);
    return;
  }

  stopProcessing();
  await refreshHistory();
  const child = recentRuns.find((run) => run.retryOfRunId === source.runId);
  if (child) {
    await openHistoryRun(child);
    return;
  }
  const failure = commandError ?? {
    code: "RETRY_FAILED",
    message: "The retry attempt could not be created.",
  };
  showFailure(source.originalFilename, failure.message, failure.code, source);
}

async function continueSelectedRun(): Promise<void> {
  const source = continuationRun;
  const runtimeAvailable = source
    && (!source.continuationRequiresRuntime || runtimeReady);
  if (!source || !source.canContinue || !runtimeAvailable || processing) {
    return;
  }

  beginProcessing(
    source,
    `Continuing ${source.originalFilename}`,
    `Resuming from ${stateLabel(source.state).toLowerCase()}…`,
  );

  let accepted: BackgroundRunAccepted | null = null;
  let commandError: CommandError | null = null;
  try {
    accepted = await invoke<BackgroundRunAccepted>("continue_document", {
      runId: source.runId,
      expectedStateVersion: source.stateVersion,
    });
  } catch (error) {
    commandError = normalizeCommandError(error);
  }

  if (accepted) {
    await monitorBackgroundRun(accepted.runId, accepted.originalFilename);
    return;
  }

  stopProcessing();
  await refreshHistory();
  const updated = recentRuns.find((run) => run.runId === source.runId);
  if (updated) {
    await openHistoryRun(updated);
    return;
  }
  const failure = commandError ?? {
    code: "CONTINUATION_FAILED",
    message: "The durable checkpoint could not be continued.",
  };
  showFailure(source.originalFilename, failure.message, failure.code, source);
}

function renderSummary(filename: string, byteSize: number, summary: SummaryArtifact): void {
  retrySourceRun = null;
  continuationRun = null;
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

function showFailure(
  title: string,
  message: string,
  code: string,
  run: RunHistoryItem | null = null,
): void {
  retrySourceRun = run?.canRetry ? run : null;
  continuationRun = run?.canContinue ? run : null;
  failureKicker.textContent = continuationRun
    ? "Durable checkpoint ready"
    : "Could not complete this document";
  failureTitle.textContent = title;
  failureMessage.textContent = message;
  failureCode.textContent = code;
  retryButton.hidden = retrySourceRun === null;
  retryHint.hidden = retrySourceRun === null && !run?.retryRunId;
  continueButton.hidden = continuationRun === null;
  continueHint.hidden = continuationRun === null;
  if (!retrySourceRun && run?.retryRunId) {
    retryHint.textContent = "A separate retry attempt already exists in Recent work.";
  }
  syncPrimaryAction();
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

function historyStateLabel(run: RunHistoryItem): string {
  if (run.failure?.code === "PROCESS_INTERRUPTED") {
    if (run.canRetry) return "Interrupted · retry available";
    if (run.retryRunId) return "Interrupted · retried";
    return "Interrupted safely";
  }
  const label = stateLabel(run.state);
  if (run.canContinue) return `${label} · continue available`;
  return run.retryOfRunId ? `${label} · retry` : label;
}

async function initialize(): Promise<void> {
  selectButton.addEventListener("click", () => void selectAndSummarize());
  runtimeRetry.addEventListener("click", () => {
    void refreshModelCatalog().then(refreshRuntimeStatus);
  });
  modelPreset.addEventListener("change", () => void selectModelPreset());
  connectActivate.addEventListener("click", () => void selectAndInstallConnectEntitlement());
  continueButton.addEventListener("click", () => void continueSelectedRun());
  retryButton.addEventListener("click", () => void retrySelectedRun());
  cancelButton.addEventListener("click", () => void cancelSelectedRun());
  historyRefresh.addEventListener("click", () => void refreshHistory());
  window.addEventListener("focus", () => void refreshConnectStatus());
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") void refreshConnectStatus();
  });
  window.setInterval(() => {
    if (document.visibilityState === "visible") void refreshConnectStatus();
  }, 30_000);
  await refreshModelCatalog();
  await Promise.all([refreshRuntimeStatus(), refreshConnectStatus(), refreshHistory()]);
  const active = recentRuns.find(isMonitoredBackgroundRun);
  if (active) {
    beginProcessing(active, active.originalFilename, processingStatusText(active));
    void monitorBackgroundRun(active.runId, active.originalFilename);
  }
}

void initialize();

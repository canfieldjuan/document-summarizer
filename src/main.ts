import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

let selectBtnEl: HTMLButtonElement | null;
let statusEl: HTMLDivElement | null;
let summaryEl: HTMLPreElement | null;
let errorEl: HTMLDivElement | null;

async function selectAndSummarize() {
  if (errorEl) errorEl.style.display = "none";
  if (statusEl) statusEl.style.display = "none";

  try {
    const selected = await open({
      multiple: false,
      filters: [{
        name: 'PDF',
        extensions: ['pdf']
      }]
    });
    
    if (selected === null) {
      return;
    }

    const filePath = selected;

    if (selectBtnEl) selectBtnEl.disabled = true;
    if (statusEl) {
      statusEl.style.display = "block";
      statusEl.querySelector("h3")!.textContent = "Summarizing…";
    }

    const result = await invoke<{ summary: { text: string } }>("summarize_document", { filePath });

    if (statusEl && summaryEl) {
      statusEl.querySelector("h3")!.textContent = "Summary";
      summaryEl.textContent = result.summary.text;
    }
  } catch (err) {
    if (errorEl) {
      errorEl.style.display = "block";
      errorEl.textContent = `Error: ${err}`;
    }
  } finally {
    if (selectBtnEl) selectBtnEl.disabled = false;
  }
}

window.addEventListener("DOMContentLoaded", () => {
  selectBtnEl = document.querySelector("#select-btn");
  statusEl = document.querySelector("#status");
  summaryEl = document.querySelector("#summary");
  errorEl = document.querySelector("#error");

  if (selectBtnEl) {
    selectBtnEl.addEventListener("click", selectAndSummarize);
  }
});

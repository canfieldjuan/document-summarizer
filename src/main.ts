import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

let selectBtnEl: HTMLButtonElement | null;
let statusEl: HTMLDivElement | null;
let resultJsonEl: HTMLPreElement | null;
let errorEl: HTMLDivElement | null;

async function selectAndIngest() {
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

    const result = await invoke("ingest_document", { filePath });
    
    if (statusEl && resultJsonEl) {
      statusEl.style.display = "block";
      resultJsonEl.textContent = JSON.stringify(result, null, 2);
    }
  } catch (err) {
    if (errorEl) {
      errorEl.style.display = "block";
      errorEl.textContent = `Error: ${err}`;
    }
  }
}

window.addEventListener("DOMContentLoaded", () => {
  selectBtnEl = document.querySelector("#select-btn");
  statusEl = document.querySelector("#status");
  resultJsonEl = document.querySelector("#result-json");
  errorEl = document.querySelector("#error");

  if (selectBtnEl) {
    selectBtnEl.addEventListener("click", selectAndIngest);
  }
});

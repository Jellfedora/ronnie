// Ronnie — Claude pour Outlook: Claude's button in Outlook's mail editor. Clicked, Claude (through Ronnie,
// with the user's Claude Code account) rephrases the draft in its tone, in the light of the mail answered;
// an instruction typed under its version has it reworked. "Replace" puts it in place of the draft (the
// signature and the quoted mail kept, Ctrl Z takes it back). Nothing is ever sent: the user sends.
(() => {
  "use strict";
  if (window.__ronnieClaude) return;
  window.__ronnieClaude = true;

  const FR = !(navigator.language || "fr").toLowerCase().startsWith("en");
  const T = FR
    ? {
        tip: "Reformuler avec Claude",
        thinking: "Claude réfléchit…",
        replace: "Remplacer le brouillon",
        copy: "Copier",
        redo: "Relancer",
        hint: "Demande autre chose à Claude : plus court, plus formel, en anglais…",
        empty: "Le brouillon est vide : écris-le d'abord.",
        failed: "Claude n'a pas répondu. Claude Code est-il installé et connecté ?",
        host: "Ronnie n'est pas prêt : active « Bouton Claude dans Outlook » dans Ronnie (Paramètres › Claude Code), puis recharge la page.",
        reload: "L'extension a été mise à jour : recharge la page.",
      }
    : {
        tip: "Rephrase with Claude",
        thinking: "Claude is thinking…",
        replace: "Replace the draft",
        copy: "Copy",
        redo: "Again",
        hint: "Ask Claude for more: shorter, more formal, in French…",
        empty: "The draft is empty: write it first.",
        failed: "Claude didn't answer. Is Claude Code installed and signed in?",
        host: "Ronnie isn't ready: turn on « Claude button in Outlook » in Ronnie (Settings › Claude Code), then reload the page.",
        reload: "The extension was updated: reload the page.",
      };

  // Where the user's own text ends in the editor: the signature, the quoted mail of a reply or a forward.
  const ENDS = '[id^="Signature"], [id*="divRplyFwdMsg"], [id^="appendonsend"], [id*="x_divRplyFwdMsg"]';
  const EDITORS = 'div[contenteditable="true"][role="textbox"], div[contenteditable="true"][aria-multiline="true"]';

  /** The mail editors on screen. */
  function editors() {
    return [...document.querySelectorAll(EDITORS)].filter((el) => {
      const r = el.getBoundingClientRect();
      return r.width > 200 && r.height > 40 && r.bottom > 0 && r.top < innerHeight;
    });
  }

  /** The editor's own part: in which container it ends, and before which of its children (none: all). */
  function bounds(editor) {
    const end = editor.querySelector(ENDS);
    if (!end) return { container: editor, cut: editor.childNodes.length, ended: false };
    // Down the wrappers that hold everything, to the level where the text and the end are siblings.
    let container = editor;
    for (;;) {
      const kids = [...container.children];
      if (kids.length === 1 && kids[0] !== end && kids[0].contains(end)) container = kids[0];
      else break;
    }
    let top = end;
    while (top.parentNode !== container) top = top.parentNode;
    return { container, cut: [...container.childNodes].indexOf(top), ended: true };
  }

  const textOf = (nodes) =>
    nodes
      .map((n) => (n.nodeType === Node.ELEMENT_NODE ? n.innerText : n.textContent))
      .join("\n")
      .replace(/\n{3,}/g, "\n\n")
      .trim();

  /** The draft (the user's own text) and what follows it (the quoted mail). */
  function parts(editor) {
    const { container, cut } = bounds(editor);
    const kids = [...container.childNodes];
    return { draft: textOf(kids.slice(0, cut)), quoted: textOf(kids.slice(cut)) };
  }

  /** The subject and the mails around (the one read, the quoted one), for Claude to understand the draft. */
  function context(editor) {
    const subject = document.querySelector(
      'input[aria-label="Objet"], input[aria-label="Ajouter un objet"], input[aria-label="Add a subject"], input[aria-label="Subject"]',
    );
    const thread = [];
    for (const body of document.querySelectorAll('[aria-label="Corps du message"], [aria-label="Message body"]')) {
      if (body === editor || body.contains(editor) || editor.contains(body) || body.isContentEditable) continue;
      const text = body.innerText.trim();
      if (text) thread.push(text.slice(0, 3000));
    }
    const { quoted } = parts(editor);
    if (quoted) thread.push(quoted.slice(0, 4000));
    return { subject: subject ? subject.value : "", thread: thread.slice(-4) };
  }

  /** `text` in place of the draft, as typed (Outlook's undo takes it back). */
  function replace(editor, text) {
    const { container, cut, ended } = bounds(editor);
    editor.focus();
    const range = document.createRange();
    range.setStart(container, 0);
    range.setEnd(container, cut);
    const selection = getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
    const typed = ended ? text + "\n\n" : text;
    if (!document.execCommand("insertText", false, typed)) {
      range.deleteContents();
      range.insertNode(document.createTextNode(typed));
      editor.dispatchEvent(new InputEvent("input", { bubbles: true }));
    }
  }

  /** Ronnie asked (through the extension's worker): { text } or { error }. */
  function ask(request) {
    return new Promise((resolve) => {
      try {
        chrome.runtime.sendMessage(request, (answer) => resolve(chrome.runtime.lastError ? { error: "reload" } : answer || { error: "empty" }));
      } catch {
        resolve({ error: "reload" });
      }
    });
  }

  // The button and the panel, in a shadow root over the page (Outlook's styles kept out).
  const host = document.createElement("div");
  host.style.cssText = "position:fixed;left:0;top:0;width:0;height:0;z-index:2147483646;";
  const root = host.attachShadow({ mode: "closed" });
  root.innerHTML = `
    <style>
      :host { all: initial; }
      * { box-sizing: border-box; font-family: -apple-system, "Segoe UI", system-ui, sans-serif; }
      .button { position: fixed; width: 28px; height: 28px; border-radius: 6px; border: none; padding: 0; cursor: pointer;
        background: transparent; display: none; align-items: center; justify-content: center; }
      .button:hover { background: rgba(127, 127, 127, 0.18); }
      .button img { width: 17px; height: 17px; display: block; }
      .panel { position: fixed; width: 420px; display: none; flex-direction: column; gap: 10px; padding: 12px;
        border-radius: 10px; border: 1px solid rgba(217, 119, 87, 0.55); background: #fff; color: #1f1f1f;
        box-shadow: 0 8px 28px rgba(0, 0, 0, 0.22); font-size: 13.5px; }
      .head { display: flex; align-items: center; justify-content: space-between; }
      .title { color: #d97757; font-weight: 600; font-size: 15px; }
      .close { border: none; background: none; font-size: 18px; cursor: pointer; color: inherit; line-height: 1; padding: 0 4px; }
      .muted { color: #6b6b6b; }
      .waiting { display: flex; align-items: center; gap: 8px; }
      .spinner { width: 14px; height: 14px; border-radius: 50%; border: 2px solid rgba(217, 119, 87, 0.3); border-top-color: #d97757;
        animation: spin 0.8s linear infinite; }
      @keyframes spin { to { transform: rotate(360deg); } }
      textarea, input { width: 100%; border: 1px solid rgba(127, 127, 127, 0.35); border-radius: 6px; padding: 7px 8px;
        font-size: 13.5px; background: transparent; color: inherit; }
      textarea { min-height: 120px; max-height: 260px; resize: vertical; }
      .row { display: flex; gap: 8px; align-items: center; }
      .pill { border: 1px solid rgba(127, 127, 127, 0.35); background: transparent; color: inherit; border-radius: 6px;
        padding: 6px 10px; font-size: 13.5px; cursor: pointer; white-space: nowrap; }
      .pill:hover { background: rgba(127, 127, 127, 0.12); }
      .main { background: #d97757; border-color: #d97757; color: #fff; }
      .main:hover { background: #c4684a; }
      @media (prefers-color-scheme: dark) {
        .panel { background: #1f1f1f; color: #eee; }
        .muted { color: #a8a8a8; }
      }
    </style>
    <button class="button" title="${T.tip}"><img alt="Claude"></button>
    <div class="panel">
      <div class="head"><span class="title">Claude</span><button class="close" title="Esc">×</button></div>
      <div class="body"></div>
    </div>`;
  const button = root.querySelector(".button");
  const panel = root.querySelector(".panel");
  const body = root.querySelector(".body");
  button.querySelector("img").src = chrome.runtime.getURL("icon.png");
  document.documentElement.appendChild(host);

  let current = null; // The editor the button is on.
  let open = null; // The panel's editor, its draft, its version, while open.

  function place() {
    if (!document.documentElement.contains(host)) document.documentElement.appendChild(host);
    const list = editors();
    if (!open) {
      const focused = list.find((e) => e.contains(document.activeElement));
      current = focused || (list.includes(current) ? current : list[list.length - 1] || null);
    }
    const editor = open ? open.editor : current;
    if (!editor || !document.contains(editor)) {
      button.style.display = "none";
      if (open) close();
      return;
    }
    const r = editor.getBoundingClientRect();
    const right = Math.min(r.right, innerWidth) - 34;
    const top = Math.max(r.top, 0) + 6;
    button.style.display = "flex";
    button.style.left = `${right}px`;
    button.style.top = `${top}px`;
    if (open) {
      panel.style.left = `${Math.max(8, Math.min(right + 28 - 420, innerWidth - 428))}px`;
      panel.style.top = `${Math.min(top + 34, innerHeight - panel.offsetHeight - 8)}px`;
    }
  }

  function close() {
    open = null;
    panel.style.display = "none";
  }

  function show(html) {
    body.innerHTML = html;
    panel.style.display = "flex";
    place();
  }

  function escape(text) {
    return text.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
  }

  async function run(instruction) {
    const state = open;
    show(`<div class="waiting"><div class="spinner"></div><span class="muted">${T.thinking}</span></div>`);
    const { subject, thread } = context(state.editor);
    const request = { kind: "mail", draft: state.draft, subject, thread };
    if (instruction && state.version) Object.assign(request, { instruction, version: state.version });
    const answer = await ask(request);
    if (open !== state) return;
    if (answer && typeof answer.text === "string" && answer.text.trim()) {
      state.version = answer.text.trim();
      answered();
    } else {
      const message = answer && answer.error === "host" ? T.host : answer && answer.error === "reload" ? T.reload : T.failed;
      show(`<div class="muted">${escape(message)}</div>`);
    }
  }

  function answered() {
    show(`
      <textarea class="answer"></textarea>
      <div class="row"><button class="pill main replace">${T.replace}</button><button class="pill copy">${T.copy}</button></div>
      <div class="row"><button class="pill redo">${T.redo}</button><input class="instruction" placeholder="${escape(T.hint)}"></div>`);
    const answer = body.querySelector(".answer");
    const instruction = body.querySelector(".instruction");
    answer.value = open.version;
    answer.addEventListener("input", () => (open.version = answer.value));
    body.querySelector(".replace").addEventListener("click", () => {
      const { editor, version } = open;
      close();
      replace(editor, version);
    });
    body.querySelector(".copy").addEventListener("click", () => {
      navigator.clipboard.writeText(open.version);
      close();
    });
    const redo = () => {
      const said = instruction.value.trim();
      if (said) run(said);
    };
    body.querySelector(".redo").addEventListener("click", redo);
    instruction.addEventListener("keydown", (e) => {
      if (e.key === "Enter") redo();
    });
  }

  // Pressed, not clicked: the editor keeps its selection and its focus until then.
  button.addEventListener("mousedown", (e) => {
    e.preventDefault();
    if (!current) return;
    if (open) return close();
    const { draft } = parts(current);
    open = { editor: current, draft, version: null };
    if (!draft) return show(`<div class="muted">${T.empty}</div>`);
    run(null);
  });
  root.querySelector(".close").addEventListener("click", close);
  root.addEventListener("keydown", (e) => {
    if (e.key === "Escape") close();
  });

  setInterval(place, 300);
  addEventListener("scroll", place, true);
  addEventListener("resize", place);
})();

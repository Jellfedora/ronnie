// Ronnie — Claude pour Outlook: Claude's button in Outlook's mail editor. Clicked, a panel asks what to do
// (anything: answer this mail saying…, rephrase my draft, sum the thread up…); Claude (through Ronnie, with
// the user's Claude Code account) answers with the conversation shown in Outlook, the subject and the
// draft as context. Its version can be changed, reworked with another instruction, then put in place of the
// draft (the signature and the quoted mail kept, the editor's undo takes it back). Nothing is ever sent:
// the user sends.
(() => {
  "use strict";
  if (window.__ronnieClaude) return;
  window.__ronnieClaude = true;

  const FR = !(navigator.language || "fr").toLowerCase().startsWith("en");
  const T = FR
    ? {
        tip: "Demander à Claude",
        ask: "Que doit faire Claude ? Ex. : réponds-lui que je suis d'accord pour jeudi, en restant poli…",
        send: "Demander",
        rephrase: "Reformuler mon brouillon",
        fix: "Corriger les fautes",
        rephraseAsk: "Reformule mon brouillon pour qu'il soit plus clair et plus fluide, en gardant son ton.",
        fixAsk: "Corrige seulement les fautes de mon brouillon (orthographe, grammaire, accents, ponctuation), sans rien changer d'autre.",
        noDraft: "Pas de brouillon : Claude écrira le mail.",
        draftSeen: (n) => `Brouillon lu (${n} caractères).`,
        threadSeen: (n) => (n ? `${n} mail${n > 1 ? "s" : ""} du fil lu${n > 1 ? "s" : ""}.` : "Aucun mail précédent trouvé."),
        thinking: "Claude réfléchit…",
        replace: "Remplacer le brouillon",
        put: "Mettre dans le mail",
        copy: "Copier",
        redo: "Relancer",
        hint: "Autre chose ? Plus court, plus formel, en anglais…",
        failed: "Claude n'a pas répondu. Claude Code est-il installé et connecté ?",
        host: "Ronnie n'est pas prêt : active « Bouton Claude dans Outlook » dans Ronnie (Paramètres › Claude Code), puis recharge la page.",
        reload: "L'extension a été mise à jour : recharge la page.",
      }
    : {
        tip: "Ask Claude",
        ask: "What should Claude do? E.g.: answer that Thursday suits me, politely…",
        send: "Ask",
        rephrase: "Rephrase my draft",
        fix: "Fix the mistakes",
        rephraseAsk: "Rephrase my draft to make it clearer and smoother, keeping its tone.",
        fixAsk: "Only fix my draft's mistakes (spelling, grammar, punctuation), changing nothing else.",
        noDraft: "No draft: Claude will write the mail.",
        draftSeen: (n) => `Draft read (${n} characters).`,
        threadSeen: (n) => (n ? `${n} mail${n > 1 ? "s" : ""} of the thread read.` : "No earlier mail found."),
        thinking: "Claude is thinking…",
        replace: "Replace the draft",
        put: "Put in the mail",
        copy: "Copy",
        redo: "Again",
        hint: "Anything else? Shorter, more formal, in French…",
        failed: "Claude didn't answer. Is Claude Code installed and signed in?",
        host: "Ronnie isn't ready: turn on « Claude button in Outlook » in Ronnie (Settings › Claude Code), then reload the page.",
        reload: "The extension was updated: reload the page.",
      };

  // Where the user's own text ends in the editor: the signature, the quoted mail of a reply or a forward.
  const ENDS = '[id^="Signature"], [id*="divRplyFwdMsg"], [id^="appendonsend"], [id*="x_divRplyFwdMsg"], [id^="mail-editor-reference-message"]';
  const EDITORS = 'div[contenteditable="true"][role="textbox"], div[contenteditable="true"][aria-multiline="true"]';

  /** The mail editors on screen. */
  function editors() {
    return [...document.querySelectorAll(EDITORS)].filter((el) => {
      const r = el.getBoundingClientRect();
      return r.width > 200 && r.height > 40 && r.bottom > 0 && r.top < innerHeight;
    });
  }

  /** A range's text as shown (its lines kept), read off screen. */
  function rangeText(range) {
    const box = document.createElement("div");
    box.style.cssText = "position:fixed;left:-99999px;top:0;width:800px;white-space:pre-wrap;";
    box.appendChild(range.cloneContents());
    document.body.appendChild(box);
    const text = box.innerText;
    box.remove();
    return text.replace(/ /g, " ").replace(/\n{3,}/g, "\n\n").trim();
  }

  /**
   * The user's own part of the editor: from its start to the first end (signature, quoted mail) that
   * leaves some text before it; the whole editor without one.
   */
  function bounds(editor) {
    const ends = [...editor.querySelectorAll(ENDS)];
    const split = (end) => {
      const before = document.createRange();
      before.setStart(editor, 0);
      before.setEndBefore(end);
      const after = document.createRange();
      after.setStartBefore(end);
      after.setEnd(editor, editor.childNodes.length);
      return { mine: before, rest: after, ended: true };
    };
    for (const end of ends) {
      const parts = split(end);
      if (rangeText(parts.mine)) return parts;
    }
    // Nothing written yet above the signature or the quoted mail: an empty place before them.
    if (ends.length) return split(ends[0]);
    const whole = document.createRange();
    whole.selectNodeContents(editor);
    return { mine: whole, rest: null, ended: false };
  }

  /** The draft (the user's own text) and what follows it (signature, quoted mail). */
  function parts(editor) {
    const { mine, rest } = bounds(editor);
    return { draft: rangeText(mine), quoted: rest ? rangeText(rest) : "" };
  }

  /** The conversation shown in Outlook (the reading pane: the mails before, with who and when), without the editor. */
  function conversation(editor) {
    const pane = document.querySelector('[aria-label="Volet de lecture"], [aria-label="Reading Pane"], [aria-label="Reading pane"]') || document.querySelector('[role="main"]');
    const texts = [];
    if (pane && !editor.contains(pane)) {
      let text = pane.innerText;
      const own = editor.innerText;
      if (own && own.trim()) text = text.replace(own, "");
      text = text.replace(/ /g, " ").replace(/\n{3,}/g, "\n\n").trim();
      if (text) texts.push(text.slice(0, 12000));
    }
    const { quoted } = parts(editor);
    if (quoted && !texts.some((t) => t.includes(quoted.slice(0, 200)))) texts.push(quoted.slice(0, 6000));
    return texts;
  }

  function subjectOf() {
    const input = document.querySelector('input[aria-label="Objet"], input[aria-label="Ajouter un objet"], input[aria-label="Add a subject"], input[aria-label="Subject"]');
    return input ? input.value : "";
  }

  /** `text` in place of the draft, as typed (Outlook's undo takes it back). */
  function replace(editor, text) {
    const { mine, ended } = bounds(editor);
    editor.focus();
    const selection = getSelection();
    selection.removeAllRanges();
    selection.addRange(mine);
    const typed = ended ? text + "\n\n" : text;
    if (!document.execCommand("insertText", false, typed)) {
      mine.deleteContents();
      mine.insertNode(document.createTextNode(typed));
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
      .panel { position: fixed; width: 460px; display: none; flex-direction: column; gap: 10px; padding: 12px;
        border-radius: 10px; border: 1px solid rgba(217, 119, 87, 0.55); background: #fff; color: #1f1f1f;
        box-shadow: 0 8px 28px rgba(0, 0, 0, 0.22); font-size: 13.5px; }
      .head { display: flex; align-items: center; justify-content: space-between; }
      .title { color: #d97757; font-weight: 600; font-size: 15px; }
      .close { border: none; background: none; font-size: 18px; cursor: pointer; color: inherit; line-height: 1; padding: 0 4px; }
      .muted { color: #6b6b6b; font-size: 12.5px; }
      .waiting { display: flex; align-items: center; gap: 8px; }
      .spinner { width: 14px; height: 14px; border-radius: 50%; border: 2px solid rgba(217, 119, 87, 0.3); border-top-color: #d97757;
        animation: spin 0.8s linear infinite; }
      @keyframes spin { to { transform: rotate(360deg); } }
      textarea, input { width: 100%; border: 1px solid rgba(127, 127, 127, 0.35); border-radius: 6px; padding: 7px 8px;
        font-size: 13.5px; background: transparent; color: inherit; }
      textarea { resize: vertical; }
      .request { min-height: 64px; max-height: 200px; }
      .answer { min-height: 140px; max-height: 300px; }
      .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
      .pill { border: 1px solid rgba(127, 127, 127, 0.35); background: transparent; color: inherit; border-radius: 6px;
        padding: 6px 10px; font-size: 13.5px; cursor: pointer; white-space: nowrap; }
      .pill:hover { background: rgba(127, 127, 127, 0.12); }
      .main { background: #d97757; border-color: #d97757; color: #fff; }
      .main:hover { background: #c4684a; }
      .grow { flex: 1; min-width: 0; }
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
  let open = null; // While the panel is open: its editor, the draft, the thread, Claude's version.

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
      panel.style.left = `${Math.max(8, Math.min(right + 28 - 460, innerWidth - 468))}px`;
      panel.style.top = `${Math.max(8, Math.min(top + 34, innerHeight - panel.offsetHeight - 8))}px`;
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

  /** The first view: what to ask, with what was read. */
  function asking() {
    const seen = `${open.draft ? T.draftSeen(open.draft.length) : T.noDraft} ${T.threadSeen(open.thread.length)}`;
    show(`
      <textarea class="request" placeholder="${escape(T.ask)}"></textarea>
      <div class="row">
        <button class="pill main send">${T.send}</button>
        ${open.draft ? `<button class="pill rephrase">${T.rephrase}</button><button class="pill fix">${T.fix}</button>` : ""}
      </div>
      <div class="muted">${escape(seen)}</div>`);
    const request = body.querySelector(".request");
    const send = () => {
      const said = request.value.trim();
      if (said) run(said);
    };
    body.querySelector(".send").addEventListener("click", send);
    request.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        send();
      }
    });
    if (open.draft) {
      body.querySelector(".rephrase").addEventListener("click", () => run(T.rephraseAsk));
      body.querySelector(".fix").addEventListener("click", () => run(T.fixAsk));
    }
    setTimeout(() => request.focus(), 0);
  }

  /** Claude asked `instruction` (on its last version, if any). */
  async function run(instruction) {
    const state = open;
    show(`<div class="waiting"><div class="spinner"></div><span class="muted">${T.thinking}</span></div>`);
    const request = { kind: "mail", draft: state.draft, subject: subjectOf(), thread: state.thread, instruction };
    if (state.version) request.version = state.version;
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
      <div class="row"><button class="pill main replace">${open.draft ? T.replace : T.put}</button><button class="pill copy">${T.copy}</button></div>
      <div class="row"><input class="instruction grow" placeholder="${escape(T.hint)}"><button class="pill redo">${T.redo}</button></div>`);
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
    open = { editor: current, draft, thread: conversation(current), version: null };
    asking();
  });
  root.querySelector(".close").addEventListener("click", close);
  root.addEventListener("keydown", (e) => {
    if (e.key === "Escape") close();
  });

  setInterval(place, 300);
  addEventListener("scroll", place, true);
  addEventListener("resize", place);
})();

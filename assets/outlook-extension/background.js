// Ronnie — Claude pour Outlook: the content script's requests passed on to Ronnie, which Chrome starts for
// each one (native messaging: only this extension may call it).
const HOST = "com.jellfedora.ronnie_claude";

chrome.runtime.onMessage.addListener((request, _sender, sendResponse) => {
  chrome.runtime.sendNativeMessage(HOST, request, (response) => {
    if (chrome.runtime.lastError) {
      sendResponse({ error: "host", detail: chrome.runtime.lastError.message });
    } else {
      sendResponse(response || { error: "empty" });
    }
  });
  // Answered later.
  return true;
});

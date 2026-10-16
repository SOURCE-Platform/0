// NM-05/NM-06 prototype: one native message at install/startup.
function probe() {
  try {
    chrome.runtime.sendNativeMessage("com.racker.zero.probe", { op: "hello", proto: 1 }, () => void chrome.runtime.lastError);
  } catch (e) { /* recorded by the host's absence of a log line */ }
}
chrome.runtime.onInstalled.addListener(probe);
chrome.runtime.onStartup.addListener(probe);
probe();

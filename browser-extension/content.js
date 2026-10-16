// Source Vault content script (spec §9.4, §9.6; Phase H). It fills only
// when the page's origin still equals the origin the fill was approved
// for (navigation races), fills by direct assignment plus input/change
// events, and never logs or stores a field value.

chrome.runtime.onMessage.addListener((msg) => {
  if (msg.type !== "fill" || window.location.origin !== msg.origin) return;
  const password = document.querySelector('input[type="password"]');
  if (!password) return;
  const form = password.form || document;
  const user = form.querySelector('input[type="email"], input[autocomplete="username"], input[type="text"]');
  const set = (el, value) => {
    el.focus();
    el.value = value;
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  };
  if (user && msg.fill.username) set(user, msg.fill.username);
  set(password, msg.fill.password);
  msg.fill = null; // gone from this frame's memory
});

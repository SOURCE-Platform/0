# Connect Bank Millennium to SOURCE — instructions for a browser agent

You are helping the owner connect their Bank Millennium (Poland) accounts to
SOURCE, a desktop app on their Mac, through **Enable Banking**. The owner is
present and will sign in whenever a login is needed.

The access is **read-only**: SOURCE can read balances and transactions. It
cannot move money.

## Rules

- **Never type a password, PIN, SMS code or other login secret.** At any
  login or confirmation screen (Enable Banking email link, Bank Millennium
  login, the Millennium mobile app approval), stop and say: "Please sign in
  now, then tell me to continue."
- **Never open, read, copy, upload or paste the contents of the `.pem`
  file.** It is a private key. You only need its **filename**.
- Do not take screenshots of, or describe, account numbers, balances or
  transactions beyond what a step needs.
- **Stop and ask the owner** before accepting terms of service, ticking a
  consent box, or clicking any button not named in these steps.
- If a page looks different from what is described here, or a step fails,
  stop. Describe what you see and wait. Do not guess.
- Use only the addresses below. Do not follow links from emails or other
  pages unless the owner confirms them.

**Where to run this:** use a browser **on the owner's Mac**. The downloaded
`.pem` file must end up on that Mac, and the final step (Part C) sends the
browser back to `127.0.0.1` on that same Mac. A cloud or remote browser
cannot finish Part C.

## Part A — Create the Enable Banking application

1. Go to <https://enablebanking.com/sign-in/>.
2. Ask the owner which email address to use, and enter it. Submit.
3. **STOP.** Say: "Enable Banking sent a sign-in link to your email. Please
   open it, then tell me to continue." The first sign-in creates the
   account automatically.
4. Go to <https://enablebanking.com/cp/applications>.
5. Click **Add a new application**. Fill in:
   - **Environment:** `Production` (**not** Sandbox).
   - **Application name:** `SOURCE personal`
   - **Allowed redirect URLs:** exactly
     `https://sourceovcourse.com/banking/callback`
     (no trailing slash; use `https`, not the local HTTP callback).
   - **Private key:** keep the default option that generates the key in the
     browser.
   - **Application description:** `Personal, non-commercial use. Reads
     my own Bank Millennium accounts into my private desktop app.`
   - **Email for data protection matters:** `adam@sourceovcourse.com`, as
     approved by the owner for this integration.
   - **Privacy URL:** `https://sourceovcourse.com/privacy/banking`.
   - **Terms URL:** `https://sourceovcourse.com/privacy/terms`.
6. Before clicking **Register**, read the filled-in form back to the owner
   and wait for "yes".
7. Click **Register**. A `.pem` file downloads.
8. Report the downloaded **filename** and the folder it was saved to
   (normally `~/Downloads`). The filename without `.pem` is the
   **application ID**: a 36-character code like
   `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`. Tell the owner this ID.
   - If the redirect URL is refused as invalid, stop and report the exact
     error message. Do not try a different URL.

## Part B — Whitelist the Millennium accounts

9. Open the application you just created (from
   <https://enablebanking.com/cp/applications>).
10. Click **Activate by linking accounts**.
11. Choose country **Poland**, then bank **Bank Millennium**. If there is a
    choice between personal and business, choose **personal**.
12. Click to continue. Enable Banking may first show its own end-user
    terms. **STOP** and let the owner read and accept them. Then the
    browser moves to Bank Millennium's website.
13. **STOP.** Say: "This is Bank Millennium's own login page. Please sign in
    and approve the request. Tell me when you're done." Confirm the address
    bar shows a Bank Millennium domain (for example `bankmillennium.pl`)
    before handing over. If it doesn't, warn the owner and do not continue.
14. If the bank asks which accounts to share, tell the owner to select
    **every account** they want in SOURCE. The owner chooses; you don't.
15. After the browser returns to Enable Banking, report what the page says,
    for example whether the application now shows as active or lists the
    linked accounts.

Part B only **whitelists** the accounts so the application may access them.
It does not connect SOURCE yet. That happens in Part C.

## Part C — Connect SOURCE (on the Mac)

These steps are in the SOURCE desktop app, not the browser. If you can't
control desktop apps, give the owner these steps to do themselves.

16. In SOURCE, open **Settings → Banking** (or the top-level **Banking**
    tab).
17. In **Enable Banking application ID**, enter the 36-character
    application ID from step 8.
18. Click **Select private key and save**. A Mac file picker opens. Choose
    the `.pem` file from step 8 (and nothing else). macOS may ask to allow
    Keychain access. The owner approves that.
19. Click **Connect Millennium**. SOURCE opens Bank Millennium's
    authorization in the Mac's default browser.
20. **STOP** again at the bank login, as in step 13. The owner signs in and
    approves.
21. Enable Banking returns the browser to
    `https://sourceovcourse.com/banking/callback`, which forwards authorization
    to `http://127.0.0.1:1431/banking/callback` on this Mac. The local page says
    authorization was received; SOURCE reports the import result separately.
    The owner has **10 minutes** from step 19 to finish, or SOURCE cancels
    the attempt. Use a rebuilt/restarted SOURCE with the HTTPS callback update.
22. **Leave SOURCE open** until the first import finishes. It pulls as much
    history as the bank allows, and the bank may only offer the full
    history right after authorization.
23. Report what SOURCE's Banking screen shows: the account names (numbers
    are masked), and any error message word for word.

## Done when

- Enable Banking shows the `SOURCE personal` application as active, with the
  Millennium accounts linked.
- SOURCE's Banking screen lists the Millennium accounts and their
  transactions.

After that, the owner should compare one month of transactions with a bank
statement before trusting the data. Automatic syncing stays off until they
turn it on in SOURCE.

## If something goes wrong

| What you see | What to do |
| --- | --- |
| Redirect URL rejected at registration | Stop. Report the exact error. |
| "Port in use" or callback error in SOURCE | Stop. Something else is using port 1431. Tell the owner. |
| Bank Millennium not listed under Poland | Stop. Report what banks are listed. |
| Bank asks for extra details or shows an error | Stop. Report the message word for word. |
| SOURCE says the key or application ID is invalid | Check the ID matches the `.pem` filename exactly. Never open the file. |

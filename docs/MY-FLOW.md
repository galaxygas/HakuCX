# My Flow

My Flow is a private chat between HakuCX and your phone for notes, links and files, like Opera's
Flow. There is no account and no server: HakuCX serves a small page on your local network, and
the phone talks to it directly. Turn it on in Settings → Synchronization → My Flow, or from the
My Flow panel.

## Connecting a phone

1. Open the My Flow panel and choose **Connect phone**.
2. Scan the QR code with the phone (on the same Wi-Fi). It opens
   `http://<this computer>:47652/` with a pairing code in the address's fragment.
3. If Windows asks whether HakuCX may accept connections, allow it on **private** networks.

The phone keeps its key in its browser's storage for that address. If the computer's local
address changes (another network, a new DHCP lease), connect the phone again. Disconnect a phone
with the × next to it; it then needs a new QR code.

Phones send notes, links and files up to 25 MB; files are saved in your downloads folder under
`My Flow`. From HakuCX, send a note, the current page (link button) or a file (upload button).

## How it is protected

The code is in `crates/haku-core/src/flow.rs` (protocol and store), `crates/hakucx/src/flow_server.rs`
(HTTP) and `ui/src/flow/` (the phone page). Tests check every rule below.

- **End-to-end encryption.** Every request and every answer is sealed with XChaCha20-Poly1305
  (RustCrypto on the computer, @noble/ciphers on the phone, since WebCrypto is unavailable on
  plain-HTTP pages). The additional data names the device and the purpose (`send`, `inbox`,
  `file <id>`, ...), so nothing sealed for one use is accepted for another.
- **The QR code works once.** It carries a pairing id and a random pairing key in the URL
  fragment, which browsers never send over the network. The phone's first message is sealed
  with that key; HakuCX answers with a new random device key sealed with it and forgets the
  pairing key. Someone who photographs the QR code later cannot use it, and unused codes expire
  after 10 minutes. (If someone else's device uses a code first, it appears in the device list,
  and your phone is told the code was already used.)
- **No replays.** Each request carries a random id, accepted once, and a time stamp no more than
  10 minutes off.
- **Keys at rest** are wrapped by Windows DPAPI in `flow.json`; the phone keeps its key in its
  browser's local storage.
- **Only when needed.** The server listens only while My Flow is on and a phone is paired or a
  QR code is showing. It serves the phone page and four sealed requests; requests that do not
  open are refused without detail, and requests from other websites (an `Origin` that is not the
  page's own) are refused.

Limits you should know:

- The page itself is served over plain HTTP on your network. Someone who can *change* your
  local network traffic while you pair (a compromised router, a hostile Wi-Fi) could serve the
  phone a modified page and learn the keys. Someone who can only *read* the traffic learns
  nothing but sizes and timing. Pair on a network you trust.
- Anything running as your Windows user can unwrap the device keys (DPAPI), as with the password
  vault.
- Received files are saved as-is; open them as you would any download.

My Flow does not go through the internet and is not part of folder sync.

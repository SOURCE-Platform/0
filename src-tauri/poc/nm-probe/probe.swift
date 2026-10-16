// NM-05 / NM-06 prototype (spec §9.2 layer 3, Phase H): how does Chrome on
// macOS spawn a native messaging host, and can the host verify its parent's
// code identity by pid? Logs findings to $NM_PROBE_LOG (or /tmp) and answers
// the extension. Synthetic, local only.

import Foundation
import Security

let logPath = ProcessInfo.processInfo.environment["NM_PROBE_LOG"] ?? "/tmp/nm-probe.log"
func log(_ s: String) {
    let line = s + "\n"
    if let h = FileHandle(forWritingAtPath: logPath) { h.seekToEndOfFile(); h.write(line.data(using: .utf8)!); h.closeFile() }
    else { FileManager.default.createFile(atPath: logPath, contents: line.data(using: .utf8)) }
}

func path(of pid: pid_t) -> String {
    var buf = [CChar](repeating: 0, count: 4096)
    return proc_pidpath(pid, &buf, 4096) > 0 ? String(cString: buf) : "?"
}

func ppid(of pid: pid_t) -> pid_t {
    var info = proc_bsdinfo()
    let n = proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, Int32(MemoryLayout<proc_bsdinfo>.size))
    return n > 0 ? pid_t(info.pbi_ppid) : -1
}

/// SecCode by pid, checked against the §9.2 designated requirement.
func check(_ pid: pid_t) -> String {
    var code: SecCode?
    let attrs = [kSecGuestAttributePid: pid] as CFDictionary
    let s1 = SecCodeCopyGuestWithAttributes(nil, attrs, [], &code)
    guard s1 == errSecSuccess, let code else { return "guest lookup failed \(s1)" }
    var req: SecRequirement?
    let text = "anchor apple generic and identifier \"com.google.Chrome\" and certificate leaf[subject.OU] = \"EQHXZ8M8AV\""
    guard SecRequirementCreateWithString(text as CFString, [], &req) == errSecSuccess, let req else { return "requirement failed" }
    var err: Unmanaged<CFError>?
    let s2 = SecCodeCheckValidityWithErrors(code, [], req, &err)
    var info: CFDictionary?
    var staticCode: SecStaticCode?
    SecCodeCopyStaticCode(code, [], &staticCode)
    if let staticCode { SecCodeCopySigningInformation(staticCode, SecCSFlags(rawValue: kSecCSSigningInformation), &info) }
    let ident = (info as? [String: Any])?[kSecCodeInfoIdentifier as String] as? String ?? "?"
    return s2 == errSecSuccess ? "VALID Chrome (identifier \(ident))" : "NOT Chrome: status \(s2) identifier \(ident)"
}

let me = getpid()
log("=== nm-probe start \(Date())")
log("argv: \(CommandLine.arguments)")
var p = me
for depth in 0..<6 {
    let parent = ppid(of: p)
    if parent <= 0 { break }
    log("ancestor[\(depth)] pid=\(parent) path=\(path(of: parent)) check=\(check(parent))")
    p = parent
}

// Native messaging: 4-byte little-endian length + JSON.
let stdin = FileHandle.standardInput
let header = stdin.readData(ofLength: 4)
if header.count == 4 {
    let n = header.withUnsafeBytes { $0.load(as: UInt32.self) }
    let msg = stdin.readData(ofLength: Int(n))
    log("message: \(String(decoding: msg, as: UTF8.self))")
    let reply = "{\"ok\":true,\"parent_check\":\"\(check(ppid(of: me)))\"}".data(using: .utf8)!
    var len = UInt32(reply.count)
    FileHandle.standardOutput.write(Data(bytes: &len, count: 4))
    FileHandle.standardOutput.write(reply)
}
log("=== nm-probe end")

import SwiftUI
import AppKit

struct LimitsConnectionView: View {
    @ObservedObject var connection: LimitsLiveConnection
    let required: Bool
    @State private var expanded: Bool
    @State private var port = ""
    @State private var credential = ""
    @State private var remember = false
    @State private var useToken = false

    init(connection: LimitsLiveConnection, required: Bool = false) {
        self.connection = connection
        self.required = required
        _expanded = State(initialValue: required)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if !connection.isConnected || required {
                Label(connection.status.text, systemImage: connection.status == .live ? "checkmark.circle" : "network")
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                if !required {
                    Button(expanded ? "Hide connection" : "Connect T3 Code…") {
                        expanded.toggle()
                        if !expanded { credential = "" }
                    }
                }
                if expanded {
                    Picker("Credential", selection: $useToken) {
                        Text("Pairing code (1 hour)").tag(false)
                        Text("Access token").tag(true)
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .onChange(of: useToken) { credential = "" }
                    if useToken {
                        Text("Run in a terminal: t3 auth session issue --scope orchestration:read --ttl 3650d --token-only")
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                    } else {
                        Text("In T3 Code: Settings → Connections → Authorized clients → Create link. Select only View environment, then copy the code.")
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    HStack {
                        Text("Local port")
                        TextField("T3 Code port", text: $port).textFieldStyle(.roundedBorder)
                            .accessibilityLabel("Local T3 Code port")
                    }
                    HStack(spacing: 6) {
                        SecureField(useToken ? "T3 Code access token" : "New T3 Code pairing code", text: $credential)
                            .textFieldStyle(.roundedBorder)
                            .accessibilityLabel(useToken ? "Read-only T3 Code access token" : "Read-only T3 Code pairing credential")
                        Button("Paste") {
                            if let pasted = NSPasteboard.general.string(forType: .string) {
                                credential = pasted
                            }
                        }
                        .buttonStyle(.bordered)
                        .keyboardShortcut("v", modifiers: .command)
                        .accessibilityLabel(useToken ? "Paste access token" : "Paste pairing code")
                    }
                    Toggle("Remember in Keychain", isOn: $remember)
                        .toggleStyle(.checkbox)
                    HStack {
                        Button(useToken ? "Connect" : "Pair & connect") {
                            let input = credential
                            credential = ""
                            if useToken {
                                connection.connect(port: Int(port) ?? 0, bearerToken: input, remember: remember)
                            } else {
                                connection.pair(port: Int(port) ?? 0, pairingCredential: input, remember: remember)
                            }
                        }
                        .disabled(credential.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        Button("Reconnect saved") { connection.reconnectSaved(port: Int(port) ?? 0) }
                    }
                    Text("Read-only · This Mac only")
                        .foregroundStyle(.secondary)
                        .help("Bar connects to 127.0.0.1. Linked environments are not included. T3 may perform discovery when a client connects.")
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .font(.system(size: 11))
        .onAppear {
            if port.isEmpty, let value = EnvironmentConfig.shared.serverRuntime?.port, (1...65535).contains(value) {
                port = String(value)
            }
        }
        .onDisappear { credential = "" }
    }
}

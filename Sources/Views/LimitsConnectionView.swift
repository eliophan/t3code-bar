import SwiftUI

struct LimitsConnectionView: View {
    @ObservedObject var connection: LimitsLiveConnection
    @State private var expanded = false
    @State private var port = ""
    @State private var credential = ""
    @State private var remember = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label(connection.status.text, systemImage: connection.status == .live ? "checkmark.circle" : "network")
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            HStack {
                Button(expanded ? "Hide connection" : "Connect T3 Code…") {
                    expanded.toggle()
                    if !expanded { credential = "" }
                }
                if connection.status != .cacheOnly {
                    Button("Disconnect & forget") {
                        credential = ""
                        connection.disconnect()
                    }
                }
            }
            .font(.system(size: 11))
            if expanded {
                Text("In T3 Code → Settings → Connections → Authorized clients, choose Create link. In Permissions, select only View environment (orchestration:read), then Create link → Share → Copy code. Paste that new code below.")
                    .fixedSize(horizontal: false, vertical: true)
                HStack {
                    Text("Local port")
                    TextField("T3 Code port", text: $port).textFieldStyle(.roundedBorder)
                        .accessibilityLabel("Local T3 Code port")
                }
                SecureField("New T3 Code pairing code", text: $credential)
                    .textFieldStyle(.roundedBorder)
                    .accessibilityLabel("Read-only T3 Code pairing credential")
                Toggle("Remember Bar token in Keychain", isOn: $remember)
                    .toggleStyle(.checkbox)
                HStack {
                    Button("Pair & connect") {
                        let input = credential
                        credential = ""
                        connection.pair(port: Int(port) ?? 0, pairingCredential: input, remember: remember)
                    }
                    .disabled(credential.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    Button("Reconnect saved") { connection.reconnectSaved(port: Int(port) ?? 0) }
                }
                Text("Only 127.0.0.1 is contacted. Includes this environment’s provider and CLI Proxy limits. Linked environments are excluded. T3 may perform its own discovery when a client connects.")
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
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

// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "T3CodeBar",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .executable(name: "T3CodeBar", targets: ["T3CodeBar"])
    ],
    dependencies: [],
    targets: [
        .executableTarget(
            name: "T3CodeBar",
            dependencies: [],
            path: "Sources",
            resources: [
                .process("Resources")
            ]
        ),
        .testTarget(name: "T3CodeBarTests", dependencies: ["T3CodeBar"], path: "Tests", exclude: ["Canary"])
    ]
)

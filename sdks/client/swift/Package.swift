// swift-tools-version:5.9
// This directory is mirrored to github.com/Torus403/notification-sender-swift on release,
// because Swift Package Manager installs from a repo whose root holds Package.swift.
import PackageDescription

let package = Package(
    name: "NotificationSender",
    platforms: [.iOS(.v15), .macOS(.v12)],
    products: [
        .library(name: "NotificationSender", targets: ["NotificationSender"]),
    ],
    targets: [
        .target(name: "NotificationSender"),
        .testTarget(name: "NotificationSenderTests", dependencies: ["NotificationSender"]),
    ]
)

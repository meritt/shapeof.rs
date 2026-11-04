// swift-tools-version: 6.2
import Foundation
import PackageDescription

let release: (url: String, checksum: String)? = nil

let local = "packages/swift/ShapeOfFFI.xcframework"
let ffi: Target
if FileManager.default.fileExists(atPath: Context.packageDirectory + "/" + local) {
    ffi = .binaryTarget(name: "ShapeOfFFI", path: local)
} else if let release {
    ffi = .binaryTarget(name: "ShapeOfFFI", url: release.url, checksum: release.checksum)
} else {
    fatalError("no released ShapeOfFFI yet: build \(local) with `cargo xtask xcframework`")
}

let package = Package(
    name: "ShapeOf",
    platforms: [.macOS(.v26)],
    products: [
        .library(name: "ShapeOf", targets: ["ShapeOf"]),
    ],
    targets: [
        ffi,
        .target(name: "ShapeOf", dependencies: ["ShapeOfFFI"], path: "packages/swift/Sources/ShapeOf"),
        .testTarget(name: "ShapeOfTests", dependencies: ["ShapeOf"], path: "packages/swift/Tests/ShapeOfTests"),
    ]
)

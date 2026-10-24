# sdks

Source for every public SDK. Server SDKs call the API to send notifications. Client SDKs run in mobile apps.

| Path                  | What                    | Publishes to                                                                     |
| --------------------- | ----------------------- | -------------------------------------------------------------------------------- |
| `server/typescript`   | TypeScript server SDK   | npm (`@notification-sender/node`)                                                |
| `server/python`       | Python server SDK       | PyPI (`notification-sender`)                                                     |
| `server/go`           | Go server SDK           | Go modules, from the mirror repo `Torus403/notification-sender-go`               |
| `server/rust`         | Rust server SDK         | crates.io (`notification-sender`)                                                |
| `client/swift`        | iOS client SDK          | Swift Package Manager, from the mirror repo `Torus403/notification-sender-swift` |
| `client/kotlin`       | Android client SDK      | Maven Central                                                                    |
| `client/react-native` | React Native client SDK | npm (`@notification-sender/react-native`)                                        |
| `client/flutter`      | Flutter client SDK      | pub.dev (`notification_sender`)                                                  |

Swift and Go are mirrored to their own public repos on release (see `.github/workflows/mirror-sdks.yml`), because SPM and `go get` install straight from git. Every other SDK publishes to its registry from here.

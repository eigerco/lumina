# Instructions to re-create the setup

- Create new project in xcode
- select iOS tab, and App icon
- choose a name, org, directory
- the project is created at this point
- make sure you have aarch64-apple-ios target installed + simulator
```
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
```
- build the ios artifacts `./node-uniffi/build-ios.sh`
- `cp ./node-uniffi/ios/lumina.xcframework $YOUR_IOS_PROJECT_DIR`
- `cp ./node-uniffi/ios/*.swift $YOUR_IOS_PROJECT_DIR/$YOUR_PROJECT_NAME/`
- Add the required apple-sdk SystemConfiguration framework
    - in XCode, click on your project in the source tree
    - select 'Build Phases' tab
    - expand 'Link Binary With Libraries' section
    - click a '+' sign and select 'SystemConfiguration' framework as well as 'lumina.xcframework'

- Everything should be ready to use the lumina inside the new app


# Building / testing with xcodebuild directly (without opening XCode)

This is mostly for CI setup. Creating app bundle is not possible without apple
Developer program and creating the credentials for the app. Commands below
skip signing, but still run building/testing

Building
```
xcodebuild clean build CODE_SIGN_IDENTITY="" CODE_SIGNING_REQUIRED=NO
```

Testing
```
xcodebuild -scheme LuminaDemo -sdk iphonesimulator -destination "platform=iOS Simulator,name=iPhone 16" test
```

# Todo

- run above build and test in CI

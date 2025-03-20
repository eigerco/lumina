# To recreate

- Create new project in xcode
- select iOS tab, and App icon
- choose a name, org, directory
- the project is created at this point
- build the ios artifacts `./node-uniffi/build-ios.sh`
- `cp ./node-uniffi/ios/lumina.xcframework $YOUR_IOS_PROJECT_DIR`
- `cp ./node-uniffi/ios/*.swift $YOUR_IOS_PROJECT_DIR/$YOUR_PROJECT_NAME/`
- Add the required apple-sdk SystemConfiguration framework
    - in XCode, click on your project in the source tree
    - select 'Build Phases' tab
    - expand 'Link Binary With Libraries' section
    - click a '+' sign and select 'SystemConfiguration' framework

- Everything should be ready to use the lumina inside the new app


# Building / testing with xcodebuild directly (without opening XCode)

This is mostly for our CI setup to test the example. Creating app bundle is not
possible without signing into apple developer program and creating some credentials
for your app. But we can execute just fine the build step and tests from cli.

Apparently this doesn't work with nix setup I created, which works for building
our node-uniffi. I just realized that because previously I didn't have this example in lumina's tree
so I was using the unnixed xcodebuild here. I can build it after `direnv block`.

Probably something to do with what clang is used. In successful builds it's
/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/clang
where when it fails it is
/usr/bin/clang
But I don't want to spend time on nix setup now, feel free to just ignore everything nix related :p

Building
```
xcodebuild clean build CODE_SIGN_IDENTITY="" CODE_SIGNING_REQUIRED=NO
```

Testing
```
xcodebuild -scheme LuminaDemo -sdk iphonesimulator -destination "platform=iOS Simulator,name=iPhone 16" test
```

# Todo

- Polishing this readme
- Perhaps simplify example swift app
- add script for copying artifacts from node-uniffi here
- run above build and test in CI

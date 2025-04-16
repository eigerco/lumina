# Instructions to re-create the setup

- Create a new kotlin project in xcode
...
- build the Android artifacts `./node-uniffi/build-android.sh`
- `cp -rv node-uniffi/app/* ~/AndroidStudioProjects/LuminaDemo/app`
- add imports to `build.gradle.kt`
```
implementation("net.java.dev.jna:jna:5.7.0@aar")
```



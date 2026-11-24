# DOMINION Voice Mobile

Native Expo development build. In this folder run `npm install`, `npm run typecheck`, then `npm run android` or `npm run ios` (Android SDK or Xcode/signing required). Expo Go is not supported because LiveKit requires native WebRTC. Open DOMINION Voice on the PC, enable its automatic conversation grant, enter the LiveKit and speech-provider settings, choose 휴대폰 연결 만들기, copy the short-lived pairing JSON privately to your phone, and paste it into this app. Enable the microphone explicitly. The app does not persist tokens or request camera access. Background mode disconnects. Device builds and real provider audio require separate validation.

Based on the official LiveKit Expo quickstart: https://docs.livekit.io/transport/sdk-platforms/expo/ .

## Standalone Android test APK (ARM64)

The `Sub3 Android voice APK` workflow uses Expo prebuild and Gradle `:app:assembleRelease` to embed the JS bundle, sign with the generated test/debug key, verify the signature, and upload a ZIP containing `DOMINION-Voice-0.2.1-arm64-test.apk` and its SHA256 checksum. No Metro or Expo Go is required. Download the artifact from the successful workflow run. This is a sideload test build, not a production distribution signing key. Existing installations signed with another key may not accept updates.

On the PC, update and start DOMINION Voice 0.2.0, enable automatic chat, open the voice service, fill the LiveKit URL and API key/secret plus Deepgram and Cartesia credentials and voice ID, and create mobile pairing. Copy the short-lived JSON privately to the phone. Install the APK, paste the pairing and connect, confirm the PC bot is present, then enable the microphone and say a short Korean sentence. Confirm the PC receives the request, runs the configured LLM and the phone receives audible speech. The APK itself does not include an LLM or server credentials. Do not share the pairing token publicly.

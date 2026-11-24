# DOMINION Voice Mobile

Native Expo development build. In this folder run `npm install`, `npm run typecheck`, then `npm run android` or `npm run ios` (Android SDK or Xcode/signing required). Expo Go is not supported because LiveKit requires native WebRTC. Open DOMINION Voice on the PC, enable its automatic conversation grant, enter the LiveKit and speech-provider settings, choose 휴대폰 연결 만들기, copy the short-lived pairing JSON privately to your phone, and paste it into this app. Enable the microphone explicitly. The app does not persist tokens or request camera access. Background mode disconnects. Device builds and real provider audio require separate validation.

Based on the official LiveKit Expo quickstart: https://docs.livekit.io/transport/sdk-platforms/expo/ .

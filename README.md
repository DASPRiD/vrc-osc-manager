# OSC Watch Linux

This application a client implementation of the OSC sender component for the
[OSC Watch VRChat accessory](https://booth.pm/en/items/3687002) aimed towards Linux users. It implements the same
functionality as the original application minus the functionality of toggling the Discord microphone.

Once started, the application will run completely in the background, showing a tray icon indicating that it's running.
Once VRChat is detected to be started, the icon will turn green and the application will start sending time updates
every 10 seconds.

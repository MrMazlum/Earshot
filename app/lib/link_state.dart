// What the link is doing, in the four words the whole app agrees on.
//
// In a file of its own because both the screen and the session state need it, and a screen that
// imports main.dart to find out what "connected" means is a screen that cannot be rendered on its
// own in a test.
//
// The distinction that matters: CONNECTED is the PC's own reply, never "we pressed Start".
// CONNECTING is a session that has heard nothing *yet*; NO ANSWER is one that has waited long
// enough that silence is a verdict. The app once showed LIVE while its audio went nowhere, and
// these four states are the fix.
enum LinkState { idle, connecting, connected, noAnswer }

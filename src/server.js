const express = require('express');
const app = express();
const { auth } = require('express-oauth2-jwt-bearer');
const path = require('path');

const port = process.env.PORT || 8080;
const frontendDir = path.join(__dirname, "..", "frontend");

const jwtCheck = auth({
  audience: 'abc123',
  issuerBaseURL: 'https://dev-3v7qe6ure8f3p1o1.us.auth0.com/',
  tokenSigningAlg: 'RS256'
});


const CallbackCode = class {
  constructor(code, tempLoginToken) {
    this.code = code;
    this.tempLoginToken = tempLoginToken;
    this.expiration = new Date(new Date().getTime() + 5*60*1000);
  }
}


function removeExpiredCallbackCodes() {
  let oldLength = callbackCodes.length;
  let now = new Date();
  callbackCodes = callbackCodes.filter(callbackCode => now < callbackCode.expiration);
  let newLength = callbackCodes.length;

  if (oldLength != newLength) { 
    console.log(`Removed expired callback codes: ${oldLength} -> ${newLength}`);
  }
}


function handleGetRoot(req, res) {
  res.send("Index Page is not set up yet. Please go to <code>/login.html</code>.");
}


function handleGetRedirected(req, res, next) {
  if (req.query.code === undefined) {
    res.send("Login failed: You do not have a return code! Please try logging in at <code>login.html</code>.");
    return;
  }

  let tempLoginToken = req.query.tempLoginToken;
  if (!(typeof tempLoginToken === 'string')) {
    res.send("Invalid Temp Login Token in Redirect URL!");
    return;
  }

  let callbackCode = new CallbackCode(req.query.code, tempLoginToken);
  removeExpiredCallbackCodes();
  callbackCodes.push(callbackCode);
  
  console.log(`Created callback code ${callbackCode} for temporary login ${tempLoginToken} (now ${callbackCodes.length}).`);
  res.send("<h1>Login Successful!</h1><p>You can safely close this tab and return to the AcornGM program.</p>");
}


function handleCheckCallback(req, res) {
  let tempLoginToken = req.query.tempLoginToken;
  if (!(typeof tempLoginToken === 'string')) {
    res.send("Invalid Temp Login Token in Query!");
    return;
  }
  removeExpiredCallbackCodes();

  console.log(tempLoginToken, callbackCodes, callbackCodes.length);

  for (let i = 0; i < callbackCodes.length; i++) {
    if (callbackCodes[i].tempLoginToken == tempLoginToken) {
      res.send(callbackCodes[i].code);
      return;
    }
  }

  res.send("no");
}


// main
let callbackCodes = [];

app.get("/auth_config.json", (req, res) => {
  res.sendFile(path.join(frontendDir, "auth_config.json"));
});

app.get("/", handleGetRoot);
app.get("/redirected/", handleGetRedirected);
app.post("/upload/mod", jwtCheck);    // enforce on upload mod (change this later)
app.get("/check_callback", handleCheckCallback);
app.use(express.static(frontendDir));

app.get('/authorized', function (req, res) {
    res.send('Secured Resource');
});


app.listen(port);
console.log('Running on port', port);


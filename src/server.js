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


const CallbackToken = class {
  constructor(accessToken, tempLoginToken) {
    this.accessToken = accessToken;
    this.tempLoginToken = tempLoginToken;
    this.expiration = new Date(new Date().getTime() + 5*60*1000);
  }
}


function removeExpiredCallbackTokens() {
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


async function handleGetRedirected(req, res, next) {
  if (req.query.code === undefined) {
    res.send("Login failed: You do not have a return code! Please try logging in at <code>login.html</code>.");
    return;
  }

  let tempLoginToken = req.query.tempLoginToken;
  if (!(typeof tempLoginToken === 'string')) {
    res.send("Invalid Temp Login Token in Redirect URL!");
    return;
  }

  let token = await convertAuthCodeToToken(req.query.code);
  if (token === null) {
    res.send("<h1>Login failed!</h1><p>Could not get access token from Auth0!</p>");
    return;
  }

  let callbackCode = new CallbackToken(token, tempLoginToken);
  removeExpiredCallbackTokens();
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
  removeExpiredCallbackTokens();

  for (let i = 0; i < callbackCodes.length; i++) {
    if (callbackCodes[i].tempLoginToken == tempLoginToken) {
      res.send(callbackCodes[i].code);
      return;
    }
  }

  res.send("no");
}


async function convertAuthCodeToToken(authCode) {
  let client_secret = process.env.CLIENT_SECRET;
  let options = {
    method: 'POST',
    headers: {
      'Content-Type': 'application/x-www-form-urlencoded',
    },
    body: new URLSearchParams({
      grant_type: 'client_credentials',
      client_id: 'hrxEwXcHs69kGHPxvlFM6FVIXeNWPAOX',
      client_secret: client_secret,
      audience: 'abc123'
    })
  };
  
  // {~~} validate access and id tokens
  // ( https://auth0.com/docs/get-started/authentication-and-authorization-flow/authorization-code-flow/add-login-auth-code-flow#response )

  const response = await fetch('https://dev-3v7qe6ure8f3p1o1.us.auth0.com/oauth/token', options);
  if (!response.ok) {
    console.error(`Bad response while converting code to token: ${response.status} - ${response.statusText}.`);
    console.warn(await response.text());
    return null;
  }

  let json = await response.json();

  return json['access_token'];
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


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


app.get("/auth_config.json", (req, res) => {
  res.sendFile(path.join(frontendDir, "auth_config.json"));
});

// enforce on upload mod
app.post("/upload/mod", jwtCheck);
app.use(express.static(frontendDir));

app.get('/authorized', function (req, res) {
    res.send('Secured Resource');
});

app.listen(port);

console.log('Running on port ', port);


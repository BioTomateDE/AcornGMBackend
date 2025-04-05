
let auth0Client = null;
const fetchAuthConfig = () => fetch("/auth_config.json");

const configureClient = async () => {
    const response = await fetchAuthConfig();
    const config = await response.json();

    auth0Client = await auth0.createAuth0Client({
        domain: config.domain,
        clientId: config.clientId
    });
};


const updateUI = async () => {
    const isAuthenticated = await auth0Client.isAuthenticated();
    // let token = await auth0Client.getTokenSilently();
    // alert(token)
  
    document.getElementById("btn-logout").disabled = !isAuthenticated;
    document.getElementById("btn-login").disabled = isAuthenticated;
};


const login = async () => {
    const urlParams = new URLSearchParams(window.location.search);
    const tempLoginToken = urlParams.get('tempLoginToken');
    if (!(typeof tempLoginToken === 'string')) {
        alert("Could not log in: Temporary login token is not set!\nThis shouldn't happen if you were redirected by the AcornGM program.");
        return;
    }

    await auth0Client.loginWithRedirect({
        authorizationParams: {
            redirect_uri: new URL(`./redirected/?tempLoginToken=${tempLoginToken}`, window.location.origin)
        }
    });
};


window.onload = async () => {
    await configureClient();
    updateUI();
}

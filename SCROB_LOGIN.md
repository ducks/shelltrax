# Scrob Login

Shelltrax now includes a login utility for easily connecting to your Scrob scrobbling server.

## Quick Start

### 1. Run the Login Utility

```bash
cargo run --bin scrob-login
```

Or if shelltrax is installed:

```bash
scrob-login
```

### 2. Enter Your Credentials

```
=== Scrob Login ===

Enter scrob server URL (e.g., https://scrob.yourdomain.com): https://scrob.example.com
Username: yourname
Password: ********

Logging in...

✓ Login successful!

Username: yourname
Admin: no

Your token:
abc123def456...

Add these to your shell environment:

export SCROB_SERVER_URL="https://scrob.example.com/graphql"
export SCROB_TOKEN="abc123def456..."
```

### 3. Set Environment Variables

The login utility will give you the exact commands to run. Choose your shell:

**Bash/Zsh:**
```bash
echo 'export SCROB_SERVER_URL="https://scrob.example.com/graphql"' >> ~/.bashrc
echo 'export SCROB_TOKEN="abc123def456..."' >> ~/.bashrc
source ~/.bashrc
```

**Nushell:**
```nushell
echo '$env.SCROB_SERVER_URL = "https://scrob.example.com/graphql"' >> ~/.config/nushell/env.nu
echo '$env.SCROB_TOKEN = "abc123def456..."' >> ~/.config/nushell/env.nu
```

Then restart your shell or source the config file.

### 4. Start Shelltrax

Now when you run shelltrax, it will automatically connect to your Scrob server and scrobble your tracks!

```bash
shelltrax
```

## Environment Variables

Shelltrax uses these environment variables for scrobbling:

- `SCROB_SERVER_URL` - The GraphQL endpoint URL (e.g., `https://scrob.example.com/graphql`)
- `SCROB_TOKEN` - Your authentication token

## Pre-filling Server URL

If you always use the same server, you can set `SCROB_SERVER_URL` before running the login utility:

```bash
export SCROB_SERVER_URL="https://scrob.example.com"
scrob-login
```

The utility will detect the existing URL and skip prompting for it.

## Creating an Account

If you don't have a Scrob account yet:

1. Visit your Scrob server web interface
2. Click "Sign Up"
3. Create an account
4. Then use `scrob-login` to get your token

Or use the Scrob API directly:

```bash
curl -X POST https://scrob.example.com/signup \
  -H "Content-Type: application/json" \
  -d '{"username":"yourname","password":"YourSecurePassword123"}'
```

## Security Notes

- Your password is only used during login and is not stored
- The token is what gets saved and used for scrobbling
- Tokens are stored in plain text in your shell config
- Keep your token secure - it provides full access to your Scrob account
- You can revoke tokens from your Scrob account settings

## Troubleshooting

### Login fails with "Invalid username or password"

- Double-check your credentials
- Make sure your Scrob account is active
- Try logging in via the web interface first

### Connection refused / Network error

- Verify the server URL is correct
- Check that your Scrob server is running
- Ensure you can reach the server (try `curl https://scrob.example.com`)

### Scrobbles not working in shelltrax

- Verify environment variables are set: `echo $SCROB_SERVER_URL`
- Check debug.log in the shelltrax directory for errors
- Make sure the token hasn't been revoked

## Manual Token Setup (Advanced)

If you already have a token from another source, you can set it directly without using the login utility:

```bash
export SCROB_SERVER_URL="https://scrob.example.com/graphql"
export SCROB_TOKEN="your-existing-token"
```

## Integration with Other Tools

The token format is compatible with any tool that uses the Scrob API. You can use the same token with:

- Shelltrax (this player)
- Web browsers (stored in cookies)
- Mobile apps
- Custom scripts

Just use the token in the `Authorization` header:

```
Authorization: Bearer your-token-here
```

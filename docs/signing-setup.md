# Signing setup

One-time work, on two sides: Apple signs the macOS disk image, Azure Artifact
Signing signs the Windows executable and installer. A side whose values are missing
is skipped with a warning in the build log, so do the two in either order.

Values go in GitHub under Settings → Secrets and variables → Actions: certificates
and credentials in **Secrets**, names and endpoints in **Variables**.

## macOS: Developer ID and notarisation

1. Join the [Apple Developer Program](https://developer.apple.com/programs/enrol/) as
   an organisation (99 USD a year) and wait for the Account Holder to be approved. A
   Developer ID certificate does not exist without the paid membership.
2. On a Mac, open Keychain Access → Certificate Assistant → *Request a Certificate
   From a Certificate Authority*, and save the signing request to disk.
3. In the developer portal, Certificates, Identifiers & Profiles → Certificates →
   **+** → *Developer ID Application*, upload the request and download the `.cer`.
4. Open the `.cer` so it lands in the login keychain, then right-click the
   certificate, *Export*, and save a `.p12` with a password. That password becomes
   the secret `MACOS_CERT_PASSWORD`.
5. Base64 the export on one line and put it in the secret `MACOS_CERT_P12`:

   ```sh
   base64 -i "Developer ID Application.p12" | pbcopy
   ```

6. Optional secret `MACOS_SIGN_IDENTITY`: the exact identity name, as in
   `Developer ID Application: Lynxby B.V. (ABCDE12345)`. Leave it unset and the
   workflow reads the first Developer ID Application identity from the `.p12`.
7. The identifier in brackets in step 6 is the team id: secret `APPLE_TEAM_ID`
   (also on the portal's Membership page).
8. Secret `APPLE_ID`: the Apple ID email address that belongs to the team.
9. Secret `APPLE_APP_PASSWORD`: at [appleid.apple.com](https://appleid.apple.com) →
   Sign-In and Security → App-Specific Passwords → **+**, name it `notarytool`, and
   use the generated password. The account's own password does not work with
   `notarytool`.
10. Remove the `.p12` and the signing request from disk once the secrets are set.

## Windows: Azure Artifact Signing

Publicly trusted code-signing keys have to live in an HSM, so there is no `.pfx` any
more. Artifact Signing (formerly Trusted Signing) holds the key and signs over the
network. The Basic plan is 9.99 USD a month per account, and Public Trust
certificates are available to organisations in the European Union.

1. Register the resource provider in the subscription: portal → Subscriptions → the
   subscription → Resource providers → `Microsoft.CodeSigning` → *Register*.
2. Create the signing account: portal → *Artifact Signing Accounts* → Create, region
   **West Europe**, pricing **Basic**. Note two values:

   - variable `ARTIFACT_SIGNING_ACCOUNT`: the account name
   - variable `ARTIFACT_SIGNING_ENDPOINT`: `https://weu.codesigning.azure.net`

3. Validate the organisation: the account → *Identity validations* → Organization →
   New identity → **Public**. Enter the legal entity name exactly as the trade
   register has it, its website, two monitored mailboxes on that domain, the business
   identifier (KvK number) and the registered address. Submitting needs the *Artifact
   Signing Identity Verifier* role. Processing takes one to thirty business days, and
   a later change means a new validation and a new certificate, so check the subject
   preview before submitting.
4. Create the certificate profile: the account → *Certificate profiles* → Create →
   **Public Trust**, name it (for example `lynxby-desktop`) and pick the completed
   identity validation as *Verified CN and O*. The profile signs only once the
   validation is **Completed** and the profile is **Active**.

   - variable `ARTIFACT_SIGNING_PROFILE`: the profile name

5. Create the app registration: Microsoft Entra ID → App registrations → New
   registration, single tenant, no redirect URI. Note the application (client) id and
   the directory (tenant) id:

   - secret `AZURE_CLIENT_ID`: the application (client) id
   - secret `AZURE_TENANT_ID`: the directory (tenant) id
   - secret `AZURE_SUBSCRIPTION_ID`: the subscription id, from the subscription's
     overview page

6. Add a federated credential to that app registration: Certificates & secrets →
   Federated credentials → Add credential → *GitHub Actions deploying Azure
   resources*, organisation `LynxbyAI`, repository `lynxby-desktop`, entity *Branch*,
   branch `main`. The subject becomes
   `repo:LynxbyAI/lynxby-desktop:ref:refs/heads/main`, and `azure/login` trades a
   GitHub OIDC token for an Azure token, so there is no client secret to store or
   rotate. If the login fails with `AADSTS700213`, the subject in the run log has to
   match this string exactly.

   A tag build presents a different subject,
   `repo:LynxbyAI/lynxby-desktop:ref:refs/tags/v0.1.0`, and federated credentials do
   not take wildcards. Add a credential per release tag, or put the build job behind
   a GitHub environment and use the environment subject for every run.

7. Assign the role: the signing account → Access control (IAM) → Add role assignment
   → *Artifact Signing Certificate Profile Signer* → *User, group, or service
   principal* → the app registration from step 5. Without it the login succeeds and
   the signing call returns 403.

The workflow asks for `id-token: write` on the build job for step 6, and requires
`azure/login@v3` and `azure/artifact-signing-action@v2`. While the six values above
are missing the Windows build logs `ARTIFACT_SIGNING_ENDPOINT,
ARTIFACT_SIGNING_ACCOUNT or ARTIFACT_SIGNING_PROFILE is not set` and uploads an
unsigned executable and installer.

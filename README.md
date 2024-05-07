# mod-images

Sick of discord cutting off attachment links in modlogs? No more! With a private
image-bin, just for your moderators, you can rest easy knowing that even a leaked
modlog will lead to no harm.

If you need this hosted, don't hesitate to [reach out](https://www.randomairborne.dev/contact/)

## Required environment variables

- `BUCKET_NAME`: S3 Bucket name
- `S3_ENDPOINT`: AWS S3 endpoint
- `S3_REGION`: S3 region- set to `auto` for R2
- `S3_ACCESS_KEY_ID`: S3 access key ID, from AWS. Needs PUT and presigned GET permissions, with CORS allowed
  for `ROOT_URL`
- `S3_SECRET_ACCESS_KEY`: S3 secret
- `REDIS_URL`: Redis URL, used only to store OAuth2 tokens
- `GUILD`: Snowflake ID of the guild you want to check `MANAGE_MESSAGES` permissions in
- `CLIENT_ID`: Discord client ID for your app
- `CLIENT_SECRET`: Discord client secret for your app
- `ROOT_URL`: The root URL this is hosted at, e.g. `https://mod-images.example.com`.

Available on Docker/GCHR:
`ghcr.io/randomairborne/mod-images:latest`

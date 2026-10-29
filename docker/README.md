# Running Flicksy in Docker

Build the image from the project root:

```sh
docker build -t flicksy .
```

Copy `docker/config.toml` to a deployment-specific file and update the LLM
endpoint, model, and optional API key source. `localhost` inside the container
refers to the container itself. Keep the server host as `0.0.0.0` if using
Docker port publishing. The container config file is mounted read-only; the
project directory must be persistent and writable by UID/GID 10001.

```sh
docker volume create flicksy-projects
docker run --rm -p 127.0.0.1:3000:3000 \
  -v flicksy-projects:/var/lib/flicksy/projects \
  -v "$PWD/flicksy-container.toml:/etc/flicksy/config.toml:ro" \
  -e FLICKSY_API_KEY \
  flicksy
```

For a bind mount, create the host directory and arrange ownership or permissions
for UID/GID 10001. Docker initializes a new named volume with the ownership of
the image directory. Configure `api_key_env` to use the environment variable,
or mount an API key file and set `api_key_file`; omit both for an unauthenticated
endpoint. Keep credentials out of the image and build context.

The agent runs shell commands with the container user's privileges. Run only in
a trusted environment; the container does not isolate individual render runs.
Use one server process per project volume. Cancelling a render stops the agent
loop, but a command already launched by it may continue running.

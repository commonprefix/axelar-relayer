# Axelar Relayer

## Components

### Subscriber

- **Description:** Monitors the chain for incoming transactions.
- **Function:** Listens for transactions on chain and publishes them to a local RabbitMQ instance for further
  processing.

### Distributor

- **Description:** Manages task distribution from the GMP API.
- **Function:** Listens for tasks from the GMP API and enqueues them into RabbitMQ for downstream processing by other
  components.

### Ingestor

- **Description:** Processes queued messages related to chain transactions and GMP API tasks.
- **Function:** Consumes and processes messages from RabbitMQ, handling tasks such as transaction verification and
  routing.

### Includer

- **Description:** Handles transaction creation on the chain.
- **Function:** Consumes tasks from RabbitMQ to create transactions on the chain, including actions like issuing refunds
  or submitting prover messages.

## Setup

### Prerequisites

Ensure the following services are installed and running on your system:

- **Redis Server**
- **RabbitMQ**

### Installation

1. **Clone the Repository**

    ```bash
    git clone https://github.com/commonprefix/axelar-relayer.git
    cd axelar-relayer/
    ```

2. **Build the Project**

   Compile the project using Cargo:

    ```bash
    cargo build --release
    ```

3. **Configure Environment Variables**

   Create a `.env` file by copying the provided template and update the necessary configurations:

    ```bash
    cp .env_template .env
    ```

   Open the `.env` file in your preferred text editor and set the environment variables.

### Running the Components

Each component can be run individually. It's recommended to use separate terminal sessions or a process manager to
handle multiple components concurrently.
Chains are run using separate binaries, so adjust the following commands accordingly:

- **Subscriber**

    ```bash
    cargo run --bin xrpl-subscriber
    ```

- **Distributor**

    ```bash
    cargo run --bin xrpl-distributor
    ```

- **Ingestor**

    ```bash
    cargo run --bin xrpl-ingestor
    ```

- **Includer**

    ```bash
    cargo run --bin xrpl-includer
    ```

## Running locally with Docker

To run the binary locally using Docker (useful for development work on deployment):

```bash
docker build -f Dockerfile.<CHAIN> . -t relayer
AWS_PROFILE=relayer-user docker run --env-file .env -v ~/.aws:/root/.aws:ro relayer <CHAIN>_<COMPONENT>
```

You can also mount files that you want to work on, so you do not have to wait for the build to complete every time.
For example:

```bash
docker build -f Dockerfile.ton . -t relayer
docker run -e AWS_PROFILE=relayer-user -e AWS_ACCOUNT_ID=490333898633 -e WORKSPACE=ton-devnet --env-file .env -v ~/.aws:/root/.aws:ro -v $(pwd)/entrypoint-ton.sh:/usr/local/bin/entrypoint.sh -v $(pwd)/deploy/scripts/fetch_secrets.sh:/usr/local/bin/fetch_secrets.sh relayer ton_subscriber
```

If you pass `AWS_ACCOUNT_ID` and `WORKSPACE`, the entrypoint will fetch secrets from AWS Secrets Manager and put them
into
`/app/relayer-base/config/config.yml`, `/app/relayer-base/certs/client.crt` and `/app/relayer-base/certs/client.key`

`WORKSPACE` corresponds to Terraform workspace name (<CHAIN>-<ENVIRONMENT>, e.g. `ton-devnet` or `xrpl-mainnet`).
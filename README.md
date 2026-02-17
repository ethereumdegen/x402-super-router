# x402-super-router

A payment-gated AI media generation service built on the [x402 protocol](https://www.x402.org/). Accepts crypto payments (ERC-20 permit signatures) via the `X-PAYMENT` header and proxies requests to [fal.ai](https://fal.ai) models for image and GIF generation.

## How it works

1. Client sends a GET request to an endpoint (e.g. `/generate_image?prompt=a+cat`)
2. Without payment, the server returns **HTTP 402** with payment requirements (token, amount, network)
3. Client signs an ERC-20 permit and retries with an `X-PAYMENT` header containing the base64-encoded payment
4. Server verifies the payment via the x402 facilitator, calls fal.ai, and returns the generated media

## Endpoints

Endpoints are configured in `endpoints.ron`. Default endpoints:

| Path | Model | Description |
|------|-------|-------------|
| `/generate_image` | `fal-ai/flux/schnell` | Generate an AI image |
| `/generate_gif` | `fal-ai/fast-animatediff/turbo/text-to-video` | Generate an animated GIF |
| `/generate_kling` | `fal-ai/kling-image/o3/text-to-image` | Generate a Kling image |

All endpoints accept a `?prompt=<text>` query parameter.

Additional routes:
- `GET /` — Human-readable service info
- `GET /api` — JSON service info

## Environment Variables

Copy `.env.example` to `.env` and fill in the required values:

```sh
cp .env.example .env
```

### Required

| Variable | Description |
|----------|-------------|
| `FACILITATOR_SIGNER` | Ethereum address of the x402 facilitator signer |
| `WALLET_ADDRESS` | Your wallet address to receive payments |
| `FAL_KEY` | API key from [fal.ai](https://fal.ai/dashboard/keys) |

### Optional

| Variable | Default | Description |
|----------|---------|-------------|
| `PORT` | `3402` | Server listen port |
| `FACILITATOR_URL` | `https://facilitator.x402.org` | x402 facilitator service URL |
| `PAYMENT_NETWORK` | `base` | Blockchain network (e.g. `base`, `ethereum`) |
| `PAYMENT_TOKEN_ADDRESS` | `0x587Cd...1B07` | ERC-20 token contract address |
| `PAYMENT_TOKEN_SYMBOL` | `STARKBOT` | Token ticker symbol |
| `PAYMENT_TOKEN_DECIMALS` | `18` | Token decimal places |
| `PAYMENT_TOKEN_NAME` | `StarkBot` | Token name (used in EIP-712 domain) |
| `PAYMENT_TOKEN_VERSION` | `1` | Token contract version |
| `PUBLIC_URL` | `http://localhost:3402` | Public base URL for returned media links |
| `ENDPOINTS_CONFIG` | `endpoints.ron` | Path to endpoints config file |
| `RUST_LOG` | `x402_super_router=debug,tower_http=debug` | Logging filter |

## Building & Running

```sh
cargo build --release
./target/release/x402-super-router
```

Or during development:

```sh
cargo run
```

## Requirements

- Rust 2024 edition
- `ffmpeg` on `PATH` (required for GIF post-processing)

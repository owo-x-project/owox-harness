FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

ARG RTK_VERSION=v0.42.3

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    bash \
    ca-certificates \
    curl \
    git \
    jq \
    openssh-client \
    pkg-config \
    ripgrep \
    shellcheck \
  && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt clippy
RUN cargo install --git https://github.com/rtk-ai/rtk --tag "${RTK_VERSION}" --locked

RUN install -d /usr/local/rtk-shims \
  && printf '%s\n' \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    '' \
    'command_name="$(basename "$0")"' \
    'real_path="${PATH//\/usr\/local\/rtk-shims:/}"' \
    'real_path="${real_path//:\/usr\/local\/rtk-shims/}"' \
    'real_path="${real_path//\/usr\/local\/rtk-shims/}"' \
    '' \
    'if [[ "${OWOX_RTK_SHIM:-1}" == "0" || "${RTK_SHIM_ACTIVE:-}" == "1" ]]; then' \
    '  PATH="$real_path" exec "$command_name" "$@"' \
    'fi' \
    '' \
    'export RTK_SHIM_ACTIVE=1' \
    'exec /usr/local/cargo/bin/rtk "$command_name" "$@"' \
    >/usr/local/rtk-shims/.rtk-shim \
  && chmod +x /usr/local/rtk-shims/.rtk-shim \
  && for command_name in git gh cargo rustc rg docker docker-compose kubectl npm npx pnpm yarn bun pytest go make; do \
    ln -s .rtk-shim "/usr/local/rtk-shims/${command_name}"; \
  done \
  && printf '%s\n' 'export PATH="/usr/local/rtk-shims:${PATH}"' >/etc/profile.d/zz-rtk-shims.sh

ENV OWOX_RTK_SHIM=1 \
  RTK_TELEMETRY_DISABLED=1

WORKDIR /workspace/product/control

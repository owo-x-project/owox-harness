FROM mcr.microsoft.com/devcontainers/base:ubuntu-24.04

ARG NODE_MAJOR=24
ENV DEBIAN_FRONTEND=noninteractive

# ---------- System packages ----------
RUN apt-get update && apt-get install -y --no-install-recommends \
  ca-certificates \
  curl \
  git \
  git-lfs \
  gh \
  jq \
  less \
  unzip \
  zip \
  sudo \
  openssh-client \
  bash-completion \
  build-essential \
  pkg-config \
  make \
  procps \
  ripgrep \
  fd-find \
  tree \
  bubblewrap \
  python3 \
  python3-venv \
  python3-pip \
  pipx \
  && rm -rf /var/lib/apt/lists/*

# ---------- JavaScript/TypeScript Dev Environment ----------
RUN mkdir -p /etc/apt/keyrings \
  && curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key \
  | gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg \
  && echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_${NODE_MAJOR}.x nodistro main" \
  > /etc/apt/sources.list.d/nodesource.list \
  && apt-get update \
  && apt-get install -y --no-install-recommends nodejs \
  && rm -rf /var/lib/apt/lists/* \
  && corepack enable \
  && corepack prepare pnpm@latest --activate

# ---------- Python Dev Environment ----------
RUN curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR=/usr/local/bin sh

# ---------- AI coding agents ----------
RUN npm install -g \
  opencode-ai \
  @openai/codex \
  @anthropic-ai/claude-code

# ---------- RTK ----------
RUN curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/refs/heads/master/install.sh \
  | RTK_INSTALL_DIR=/usr/local/bin sh

RUN mkdir -p \
  /home/vscode/.claude \
  /home/vscode/.config/opencode \
  /home/vscode/.local/share/opencode \
  /home/vscode/.cache/opencode \
  /home/vscode/.npm-global \
  /home/vscode/.local/bin \
  /home/vscode/.local/share/pnpm \
  /home/vscode/.local/share/opencode \
  /home/vscode/.cache/uv \
  /home/vscode/.cache/pip \
  /home/vscode/.cache/opencode \
  /pnpm/store \
  && chown -R vscode:vscode /home/vscode /pnpm

ENV NPM_CONFIG_PREFIX=/home/vscode/.npm-global
ENV PNPM_HOME=/home/vscode/.local/share/pnpm
ENV PNPM_STORE_DIR=/pnpm/store
ENV UV_CACHE_DIR=/home/vscode/.cache/uv
ENV PIP_CACHE_DIR=/home/vscode/.cache/pip
ENV PIPX_HOME=/home/vscode/.local/pipx
ENV PIPX_BIN_DIR=/home/vscode/.local/bin
ENV HOME=/home/vscode
ENV CLAUDE_CONFIG_DIR=/home/vscode/.claude
ENV PATH=/home/vscode/.npm-global/bin:/home/vscode/.local/bin:/home/vscode/.local/share/pnpm:/usr/local/bin:${PATH}

WORKDIR /workspace

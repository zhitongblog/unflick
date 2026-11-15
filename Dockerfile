# Dockerfile for MCP registry health checks (Glama, Smithery, etc.).
#
# Runs the stdio MCP launcher with the full compiled-in surface (114 tools,
# 3 resources) so registry introspection (`initialize`, `tools/list`,
# `resources/list`) succeeds headlessly without X11/Wayland/libmpv GPU deps,
# while `tools/call` returns clear desktop install instructions.
#
# End users install the native desktop app from https://unflick.app.

FROM node:20-alpine
WORKDIR /app
COPY marketplace/mcp/npm-wrapper/ ./
ENV UNFLICK_MCP_FORCE_STUB=1
ENTRYPOINT ["node", "/app/bin/unflick-mcp.mjs"]

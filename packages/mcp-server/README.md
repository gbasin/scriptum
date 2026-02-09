# @scriptum/mcp-server

MCP stdio server for Scriptum.

## Run

```bash
npx -y @scriptum/mcp-server
```

From the monorepo:

```bash
pnpm --filter @scriptum/mcp-server run build
node packages/mcp-server/dist/index.js
```

## Claude Code Configuration

```json
{
  "mcpServers": {
    "scriptum": {
      "command": "npx",
      "args": ["-y", "@scriptum/mcp-server"]
    }
  }
}
```

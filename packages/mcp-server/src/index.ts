#!/usr/bin/env node
import { createServer } from "./server.js";

const server = createServer();

void server.start();

# Project Guidelines for AI Assistants

This document provides guidelines for AI assistants working on this codebase. Please adhere to these instructions to ensure efficient and accurate interaction with the project.

## Prioritizing Codebase Memory MCP

**IMPORTANT:** For all tasks involving codebase understanding, structural analysis, searching for code elements, tracing call paths, or getting architectural overviews, you **MUST** prioritize the use of the `codebase-memory-mcp` tool. This tool provides a highly optimized knowledge graph of the codebase, significantly reducing token usage and improving the accuracy and speed of your responses compared to file-by-file exploration.

Avoid using generic file search (e.g., `grep`, `find`) or reading multiple files directly for structural information if `codebase-memory-mcp` can provide the answer.

## How to Use Codebase Memory MCP

Here are common scenarios and the preferred `codebase-memory-mcp` Qtools to use:

### 1. Initial Project Indexing

If the project has not been indexed or if you suspect the graph is outdated after a major `git pull`, initiate an indexing process.

**Your Prompt:** "Index this project."

*(This will trigger `codebase-memory-mcp.index_repository` automatically. The graph will auto-sync after the initial index.)*

### 2. Getting Codebase Architecture Overview

To understand the high-level structure, languages, packages, or architectural patterns.

**Your Prompt:** "Provide an overview of the project's architecture."
**Your Prompt:** "What are the main languages and packages used here?"
**Your Prompt:** "Identify any architectural hotspots or boundaries."

*(These prompts should trigger `codebase-memory-mcp.get_architecture`.)*

### 3. Searching for Code Elements (Functions, Classes, Routes, etc.)

To find specific functions, classes, methods, or routes based on names or patterns.

**Your Prompt:** "Find all functions related to user authentication."
**Your Prompt:** "List all `Controller` classes in the project."
**Your Prompt:** "Show me all defined API routes."

*(These prompts should trigger `codebase-memory-mcp.search_graph`.)*

### 4. Tracing Call Paths and Dependencies

To understand what a function calls, what calls a function, or to analyze the impact of a change.

**Your Prompt:** "What functions call `ProcessOrder`?"
**Your Prompt:** "Show me the call chain for `HandleRequest`."
**Your Prompt:** "Analyze the potential impact if I modify the `UpdateUser` function."

*(These prompts should trigger `codebase-memory-mcp.trace_call_path`.)*

### 5. Detecting Dead Code

To identify unused functions or code segments.

**Your Prompt:** "Find any dead code in the codebase."
**Your Prompt:** "Are there any functions that are never called?"

*(These prompts should trigger `codebase-memory-mcp.search_graph` with appropriate filters.)*

### 6. Executing Advanced Graph Queries

For complex structural queries that might require more specific graph traversal logic.

**Your Prompt:** "Run a Cypher query to find all functions called by `main`."
**Your Prompt:** "Execute a graph query to list HTTP calls with high confidence."

*(These prompts should trigger `codebase-memory-mcp.query_graph`.)*

### 7. General Text Search within Files

If your query is purely text-based and does not require structural understanding (e.g., searching for comments, specific strings not tied to code structure), you may use `codebase-memory-mcp.search_code`.

**Your Prompt:** "Search for all 'TODO' comments in the project."

*(This prompt should trigger `codebase-memory-mcp.search_code`.)*

## General Guidelines

*   **Be Specific:** When asking questions, be as specific as possible about what you are looking for. This helps the AI assistant choose the most appropriate tool.
*   **Verify Results:** Always critically evaluate the results provided by the AI assistant, especially for complex queries.
*   **Provide Context:** If a query is ambiguous, provide additional context or file paths to help the AI assistant narrow down its search.

By following these guidelines, you will enable a more efficient and intelligent interaction with this codebase.

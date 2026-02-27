# LiasionDB API Documentation

LiasionDB is a graph-based version control database that tracks complete provenance of content. It provides a file-like API for LLMs and users with automatic provenance tracking.

## Starting the Server

```bash
cargo run
```

The server listens on `http://127.0.0.1:3000` by default.

You can configure the file storage directory with the `FILE_DIR` environment variable:

```bash
FILE_DIR=./my_files cargo run
```

## How Provenance Tracking Works

LiasionDB uses a single **`.ledger`** file to track what you've read:

1. **Reading files** appends their node IDs to `.ledger`
2. **Writing files** uses all node IDs in `.ledger` as references
3. This creates a provenance chain: "I wrote X while reading Y and Z"

The `.ledger` file is shared across the session - it accumulates all reads until you clear it.

## API Endpoints

### 1. Health Check

**GET** `/health`

Check if the server is running.

**Response:**
```
OK
```

**Example:**
```bash
curl http://127.0.0.1:3000/health
```

---

### 2. List Files

**GET** `/files`

Lists all files in the knowledge base.

**Response:**
```json
["example.md", "docs/readme.md"]
```

**Example:**
```bash
curl http://127.0.0.1:3000/files
```

---

### 3. Read File

**GET** `/files/{filepath}`

Reads a file from the knowledge base and:
- Saves it to `{FILE_DIR}/{filepath}`
- **Appends** the node IDs to `{FILE_DIR}/.ledger`

**Response:**
- Content-Type: text/plain
- Body: The file content (HTML representation of the markdown)

**Side Effects:**
- Creates/overwrites `{FILE_DIR}/{filepath}` with the file content
- **Appends** node IDs to `{FILE_DIR}/.ledger` (creates if doesn't exist)

**Example:**
```bash
curl http://127.0.0.1:3000/files/example.md
```

This saves to `./files/example.md` and appends node IDs to `./files/.ledger`

---

### 4. Write File

**POST** `/files/{filepath}`

Writes a file to the knowledge base. All node IDs in `.ledger` are used as reference nodes to track what was read before writing.

**Request Body:**
```json
{
  "content": "# My Document\n\nThis is my content."
}
```

**Response:**
```json
{
  "status": "success",
  "file_idx": 5
}
```

**How it Works:**
1. Reads `{FILE_DIR}/.ledger` to get all previously read node IDs
2. Converts those node IDs back to Node objects
3. Creates a file node linked to the appropriate directory
4. Converts markdown to HTML and creates content nodes
5. Links all new content nodes to the reference nodes in the `ref_table`

**Example:**
```bash
curl -X POST http://127.0.0.1:3000/files/my-doc.md \
  -H "Content-Type: application/json" \
  -d '{"content": "# Hello World\n\nThis is my document."}'
```

---

### 5. Clear Ledger

**DELETE** `/ledger`

Clears the `.ledger` file, removing all tracked node IDs.

Use this when you want to start a fresh provenance tracking session.

**Response:**
```json
{
  "status": "ledger cleared"
}
```

**Example:**
```bash
curl -X DELETE http://127.0.0.1:3000/ledger
```

---

## Example Workflows

### Workflow 1: Simple Read and Write

```bash
# Read existing documentation (accumulates node IDs in .ledger)
curl http://127.0.0.1:3000/files/design.md > design.md

# Write new implementation (references nodes from design.md)
curl -X POST http://127.0.0.1:3000/files/implementation.md \
  -H "Content-Type: application/json" \
  -d '{"content": "# Implementation\n\nBased on the design doc..."}'
```

The database now knows `implementation.md` was written while referencing `design.md`!

---

### Workflow 2: Multiple References

```bash
# Clear ledger to start fresh
curl -X DELETE http://127.0.0.1:3000/ledger

# Read multiple files (all accumulate in .ledger)
curl http://127.0.0.1:3000/files/design.md > /dev/null
curl http://127.0.0.1:3000/files/api-spec.md > /dev/null
curl http://127.0.0.1:3000/files/requirements.md > /dev/null

# Write new file (references ALL three files)
curl -X POST http://127.0.0.1:3000/files/implementation.md \
  -H "Content-Type: application/json" \
  -d '{"content": "# Implementation\n\nBased on design, API spec, and requirements..."}'
```

The database tracks that `implementation.md` was written while reading all three documents!

---

### Workflow 3: Iterative Writing

```bash
# Clear ledger
curl -X DELETE http://127.0.0.1:3000/ledger

# Read source material
curl http://127.0.0.1:3000/files/research.md > /dev/null

# Write first draft
curl -X POST http://127.0.0.1:3000/files/article.md \
  -H "Content-Type: application/json" \
  -d '{"content": "# Article Draft 1\n\nFirst attempt..."}'

# Clear ledger for next iteration
curl -X DELETE http://127.0.0.1:3000/ledger

# Read your own draft
curl http://127.0.0.1:3000/files/article.md > /dev/null

# Write revision (now references the previous draft)
curl -X POST http://127.0.0.1:3000/files/article-v2.md \
  -H "Content-Type: application/json" \
  -d '{"content": "# Article Draft 2\n\nImproved version..."}'
```

---

## Ledger File Format

The `.ledger` file is a simple JSON file:

```json
{
  "node_indices": [2, 5, 7, 12, 15, 18]
}
```

- Node IDs are unique (stored in IndexSet)
- Duplicates are automatically removed
- Order is preserved

---

## Directory Structure

The database automatically creates directory nodes:

```
POST /files/docs/readme.md     → Creates DIR:docs → FILE:docs/readme.md → content nodes
POST /files/src/main.rs        → Creates DIR:src → FILE:src/main.rs → content nodes
```

---

## Error Responses

- `404 Not Found` - File doesn't exist in the knowledge base
- `500 Internal Server Error` - Server error (e.g., failed to write to disk)

---

## For LLMs

This API is designed to work like a file system with automatic provenance tracking:

1. **GET /files** - See what files are available
2. **GET /files/{path}** - Read files (automatically tracks what you read)
3. **POST /files/{path}** - Write files (automatically links to what you read)
4. **DELETE /ledger** - Clear your reading history to start fresh

The ledger tracking is **completely automatic** - just read and write files normally, and the database tracks the provenance graph for you!

Think of it like this:
- The `.ledger` file is your "reading list" 
- Every file you read gets added to the list
- Every file you write gets linked to everything on the list
- Clear the list when you want to start a new context

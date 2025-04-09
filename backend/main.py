# backend/main.py
import os
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from dotenv import load_dotenv

# Load environment variables from .env file in the backend directory
# Note: Docker compose `env_file` also injects vars, but this ensures they are loaded if script run directly too
load_dotenv()

app = FastAPI(title="RAG Backend API")

# Get allowed origins from environment or default to localhost:3000
# In production, set this via environment variables more securely
allowed_origins = [
    "http://localhost:3000", # Default Next.js dev port
    # Add other origins if needed, e.g., your production frontend URL
]

app.add_middleware(
    CORSMiddleware,
    allow_origins=allowed_origins,
    allow_credentials=True,
    allow_methods=["*"], # Allows all methods
    allow_headers=["*"], # Allows all headers
)

@app.get("/")
async def read_root():
    """ Basic endpoint to check if the API is running """
    return {"message": "RAG Backend API is running!"}

# Placeholder for the query endpoint
@app.post("/query")
async def handle_query(query_data: dict):
    # TODO: Implement RAG logic here
    # 1. Receive query (e.g., query_data['query'])
    # 2. Embed query
    # 3. Search pgvector
    # 4. Search Neo4j (optional context)
    # 5. Call LLM with context
    # 6. Return response
    query = query_data.get("query", "No query provided")
    print(f"Received query: {query}") # Basic logging
    return {"answer": f"Processing query: '{query}'. RAG logic not yet implemented.", "sources": []}

# Add other endpoints as needed (e.g., for ingestion)

# Optional: Run directly for simple testing (though docker compose command is preferred)
# if __name__ == "__main__":
#     import uvicorn
#     port = int(os.getenv("FASTAPI_PORT", 8001))
#     uvicorn.run(app, host="0.0.0.0", port=port)

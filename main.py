from fasthtml.common import *
from fastlite import *
from fastcore.utils import *
from fastcore.net import urlsave

# Initialize the FastHTML app
app, rt = fast_app()

# Database connection
db = database("roc_corpus.db")

VALID_STATIC_FILES = {
    "roc_wasm_parse_bg.wasm",
    "roc_wasm_parse_bg.wasm.d.ts",
    "roc_wasm_parse.d.ts",
    "roc_wasm_parse.js",
}

@rt("/static/{filename:path}")
def static(filename: str):
    if filename in VALID_STATIC_FILES:
        return FileResponse(f'/home/wwwpython/osprey/wasm/roc-wasm-parse/pkg/{filename}')
    else:
        return Response("Not Found", status_code=404)

# Define the route for the homepage
@rt("/")
def get_home():
    return Titled("Database Viewer",
        Ul(
            Li(A("View ROC Files", href="/roc_files")),
            Li(A("View Repo Scan Results", href="/repo_scan_results")),
        ))

# Define the route to list `roc_files`
@rt("/roc_files")
def get_roc_files():
    items = [
        Li(A(f"{row.id}: {row.file_path}", href=f"/roc_files/{row.id}"))
        for row in db.q(f"SELECT id, file_path FROM roc_files")
    ]
    return Titled("ROC Files", Ul(*items))

# Define the route to view a single `roc_files` entry
@rt("/roc_files/{id}")
def get_roc_file(id: int):
    try:
        row = db.q(f"SELECT * FROM roc_files WHERE id = {id}")[0]
        return Titled(f"ROC File ID: {row["id"]}",
            Ul(
                Li(f"File Hash: {row["file_hash"]}"),
                Li(f"Commit SHA: {row["commit_sha"]}"),
                Li(f"Retrieval Date: {row["retrieval_date"]}"),
                Li(f"File Path: {row["file_path"]}"),
                Li(f"Repo URL: {row["repo_url"]}"),
                Li(f"File Contents:"),
                Pre(Code(row["file_contents"])),
            ))
    except NotFoundError:
        return Titled("Error", P("ROC File not found"))

# Define the route to list `repo_scan_results`
@rt("/repo_scan_results")
def get_repo_scan_results():
    items = [
        Li(A(f"{row.id}: {row.repo_url}", href=f"/repo_scan_results/{row.id}"))
        for row in repo_scan_results.all()
    ]
    return Titled("Repo Scan Results", Ul(*items))

# Define the route to view a single `repo_scan_results` entry
@rt("/repo_scan_results/{id}")
def get_repo_scan_result(id: int):
    try:
        row = repo_scan_results[id]
        return Titled(f"Repo Scan ID: {row.id}",
            Ul(
                Li(f"Repo URL: {row.repo_url}"),
                Li(f"Scan Date: {row.scan_date}"),
                Li(f"Scan SHA: {row.scan_sha}"),
                Li(f"Scan Results: {row.scan_results}"),
            ))
    except NotFoundError:
        return Titled("Error", P("Repo Scan Result not found"))

# Serve the application
serve()

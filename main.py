from fasthtml.common import *
from fastlite import database, NotFoundError

# Initialize the FastHTML app
app = FastHTML()
rt = app.route

# Database connection
db = database("roc_corpus.db")

WASM_FILES = {
    "roc_wasm_parse_bg.wasm",
    "roc_wasm_parse_bg.wasm.d.ts",
    "roc_wasm_parse.d.ts",
    "roc_wasm_parse.js",
}

css = Style("""
* {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
}

html,
body {
    height: 100%;
    overflow: hidden;
}

#header-style {
    flex-shrink: 0;
    background-color: #333;
    color: white;
    padding: 10px;
}

#header.collapsed {
    height: 0;
    overflow: hidden;
    padding: 0;
}

#container {
    display: flex;
    height: 100vh;
    transition: height 0.3s;
}

#left-column {
    background-color: #f4f4f4;
}

#right-column {
    background-color: #ddd;
}

#right-column.hidden {
    display: none;
}

#toggle-right {
    position: absolute;
    top: 10px;
    right: 10px;
    background-color: #555;
    color: white;
    border: none;
    padding: 5px 10px;
    cursor: pointer;
}

#toggle-header {
    background-color: #555;
    color: white;
    border: none;
    padding: 5px 10px;
    cursor: pointer;
}
""")

@rt("/static/{filename:path}")
def get_static(filename: str):
    print("BLARG!" + filename + f" {filename in WASM_FILES}")
    if filename in WASM_FILES:
        return FileResponse(f'wasm/roc-wasm-parse/pkg/{filename}')
    else:
        return Response("Not Found", status_code=404)
# @rt("/{fname:path}")
# def static(fname:str):
#     raise ValueError(f"BLARG! {fname}.{ext}")
#     return FileResponse(f'{fname}.{ext}')


# Define the route for the homepage
@rt("/")
def get_home():
    return Titled("Database Viewer",
        Ul(
            Li(A("View ROC Files", href="/files")),
            Li(A("View Repo Scan Results", href="/repo_scan_results")),
        ))

# Define the route to list `roc_files`
@rt("/files")
def get_roc_files():
    rows = db.q(f"""
    SELECT id, retrieval_date, length(file_contents) as length, repo_url, file_path
    FROM roc_files
    WHERE not (repo_url like '%/roc')
    """)
    table_rows = [
        Tr(
            Td(A(f"{row["id"]}", href=f"/files/{row["id"]}")),
            Td(row["retrieval_date"]),
            Td(row["length"]),
            Td(row["repo_url"]),
            # syncing emjoi unicode
            Td("🔄" if row["length"] > 0 else "❌"),
            Td(row["file_path"])
        )
        for row in rows
    ]
    table = Table(
        Tr(
            Th("ID"),
            Th("Retrieval Date"),
            Th("Length"),
            Th("Repo URL"),
            Th("Parse Result"),
            Th("File Path")
        ),
        id="rocTable",
        *table_rows,
    )
    js = """
    import init, { parse_and_debug } from '/static/roc_wasm_parse.js';

    let parser_ready = (async () => {
      // Initialize the WASM module (this fetches and instantiates the .wasm file)
      await init();
    })();

    document.addEventListener("DOMContentLoaded", () => {
        const rows = document.querySelectorAll("#rocTable tr");

        parser_ready.then(() => {
            const observer = new IntersectionObserver((entries, observer) => {
                entries.forEach(entry => {
                    if (entry.isIntersecting) {
                    const row = entry.target;
                    let parseResultCell = null;
                    let idCell = null;

                    // Manually iterate over the cells to find the relevant nodes
                    const cells = row.getElementsByTagName("td");
                    for (let i = 0; i < cells.length; i++) {
                        if (i === 4) {
                            parseResultCell = cells[i];
                        } else if (i === 0) {
                            const links = cells[i].getElementsByTagName("a");
                            if (links.length > 0) {
                                idCell = links[0];
                            }
                        }
                    }

                    if (parseResultCell && parseResultCell.textContent === "🔄") {
                        const id = idCell ? idCell.textContent : null;

                        if (id) {
                            fetch(`/content/${id}`)
                                .then(response => response.text())
                                .then(roc_code => {
                                    let result = parse_and_debug(roc_code);
                                    if (result.startsWith("Full {")) {
                                        parseResultCell.textContent = "✅";
                                    } else {
                                        // red circle emoji
                                        parseResultCell.textContent = "🔴";
                                    }
                                })
                                .catch(error => console.error("Error parsing file:", error));
                            }
                        }
                        observer.unobserve(row);
                    }
                });
            });

            rows.forEach(row => observer.observe(row));
        });
    });
    """
    return Titled("ROC Files", table, Script(code=js, type="module"))

# Define the route to view a single `roc_files` entry
@rt("/files/{id}")
def get_roc_file(id: int):
    try:
        row = db.q(f"SELECT * FROM roc_files WHERE id = {id}")[0]
    except NotFoundError:
        return Titled("Error", P("ROC File not found"))

    js = """
    import init, { parse_and_debug } from '/static/roc_wasm_parse.js';

    (async () => {
      // Initialize the WASM module (this fetches and instantiates the .wasm file)
      await init();

      // Read the contents of the roc_code element
      const roc_code = document.getElementById("roc_code").textContent;

      // Now you can call the function
      const result = parse_and_debug(roc_code);

      // Display the result in the parse_output element
      document.getElementById("parse_output").textContent = result;
    })();

    const header = document.getElementById("header");
    const container = document.getElementById("container");
    const toggleRightButton = document.getElementById("toggle-right");
    const toggleHeader = document.getElementById("toggle-header");
    const rightColumn = document.getElementById("right-column");

    toggleHeader.addEventListener("click", () => {
        header.classList.toggle("collapsed");
        container.classList.toggle("collapsed");
    });

    toggleRightButton.addEventListener("click", () => {
        rightColumn.classList.toggle("hidden");
    });
    """


    header = Div(
        H1(f"ROC File ID: {row["id"]}"),
        Ul(
            Li(f"File Hash: {row["file_hash"]}"),
            Li(f"Commit SHA: {row["commit_sha"]}"),
            Li(f"Retrieval Date: {row["retrieval_date"]}"),
            Li(f"File Path: {row["file_path"]}"),
            Li(f"Repo URL: {row["repo_url"]}"),
        )
    )

    left = Pre(Code(row["file_contents"], id="roc_code"))
    right = Pre(id="parse_output")

    title = Title(f"ROC File ID: {row['id']}")

    return title, Main(
        css,
        columns(header, left, right),
        Script(type='module', code=js),
    )

def columns(header, left, right):
    return Div(
        Div(
            Div(header, id="header"),
            Button("Toggle Header", id="toggle-header"),
            style = "flex-shrink: 0; background-color: #333; color: white;"
        ),
        Button("Toggle Right Column", id="toggle-right"),
        Div(
            Div(
                left,
                id="left-column",
                style="flex: 1; overflow-y: auto; padding: 20px;"
            ),
            Div(
                right,
                id="right-column",
                style="flex: 1; overflow-y: auto; padding: 20px;"
            ),
            style="display: flex; flex: 1; overflow: hidden;"
        ),
        style = "display: flex; flex-direction: column; height: 100vh;"
    )

@rt("/content/{id}")
def get_content(id: int):
    try:
        row = db.q(f"SELECT file_contents FROM roc_files WHERE id = {id}")[0]
    except NotFoundError:
        return Titled("Error", P("ROC File not found"))

    return Response(row["file_contents"], media_type="text/plain")


# Define the route to list `repo_scan_results`
@rt("/repo_scan_results")
def get_repo_scan_results():
    repo_scan_results = db.q("""
    SELECT id, repo_url, max(scan_date) as last_scan_date, group_concat(distinct scan_results) as scan_results
    FROM repo_scan_results
    group by repo_url
    """)
    table_rows = [
        Tr(
            Td(A(f"{row['id']}", href=f"/repo_scan_results/{row['id']}")),
            Td(row["repo_url"]),
            Td(row["last_scan_date"]),
            Td(", ".join(row["scan_results"]))
        )
        for row in repo_scan_results
    ]
    table = Table(
        Tr(
            Th("ID"),
            Th("Repo URL"),
            Th("Last Scan Date"),
            Th("Scan Results"),
        ),
        id="repoScanTable",
        *table_rows,
    )

    return Titled("Repo Scan Results", table)

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
serve(host="127.0.0.1", port=9090)

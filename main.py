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
    background-color: #333;
    color: white;
    padding: 10px;
    cursor: pointer;
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

#left-column,
#right-column {
    flex: 1;
    overflow-y: scroll;
    height: 100vh;
    padding: 10px;
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
    rows = db.q(f"SELECT id, retrieval_date, length(file_contents) as length, repo_url, file_path FROM roc_files")
    table_rows = [
        Tr(
            Td(A(f"{row["id"]}", href=f"/files/{row["id"]}")),
            Td(row["retrieval_date"]),
            Td(row["length"]),
            Td(row["repo_url"]),
            Td(row["file_path"])
        )
        for row in rows
    ]
    table = Table(
        Tr(
            Th("ID", onclick="sortTable(0)"),
            Th("Retrieval Date", onclick="sortTable(1)"),
            Th("Length", onclick="sortTable(2)"),
            Th("Repo URL", onclick="sortTable(3)"),
            Th("File Path", onclick="sortTable(4)")
        ),
        id="rocTable",
        *table_rows,
    )
    js = """
    function sortTable(n) {
        var table, rows, switching, i, x, y, shouldSwitch, dir, switchcount = 0;
        table = document.getElementById("rocTable");
        switching = true;
        dir = "asc";
        while (switching) {
            switching = false;
            rows = table.rows;
            for (i = 1; i < (rows.length - 1); i++) {
                shouldSwitch = false;
                x = rows[i].getElementsByTagName("TD")[n];
                y = rows[i + 1].getElementsByTagName("TD")[n];
                if (dir == "asc") {
                    if (x.innerHTML.toLowerCase() > y.innerHTML.toLowerCase()) {
                        shouldSwitch = true;
                        break;
                    }
                } else if (dir == "desc") {
                    if (x.innerHTML.toLowerCase() < y.innerHTML.toLowerCase()) {
                        shouldSwitch = true;
                        break;
                    }
                }
            }
            if (shouldSwitch) {
                rows[i].parentNode.insertBefore(rows[i + 1], rows[i]);
                switching = true;
                switchcount++;
            } else {
                if (switchcount == 0 && dir == "asc") {
                    dir = "desc";
                    switching = true;
                }
            }
        }
    }
    """
    return Titled("ROC Files", table, Script(code=js))

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


    header = Titled(
        f"ROC File ID: {row["id"]}",
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

    return Div(
        css,
        columns(header, left, right),
        Script(type='module', code=js),
    )

def columns(header, left, right):
    return Div(
        Div(
            Div(header, id="header"),
            Button("Toggle Header", id="toggle-header"),
            id="header-style"
        ),
        Button("Toggle Right Column", id="toggle-right"),
        Div(
            Div(
                left,
                id="left-column"
            ),
            Div(
                right,
                id="right-column"
            ),
            id="container"
        )
    )

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

const droparea = document.getElementById("drop-area");

async function handleDrop(event) {
  const loader = document.createElement("span");
  loader.classList.add("loader");
  droparea.replaceChildren(loader);
  let files = [];
  if (event.dataTransfer.items) {
    [...event.dataTransfer.items].forEach((item, i) => {
      if (item.kind === "file") {
        const file = item.getAsFile();
        files = [...files, file];
      }
    });
  } else {
    [...event.dataTransfer.files].forEach((file, i) => {
      files = [...files, file];
    });
  }
  if (files.length !== 1) {
    alert("You may only upload one file at a time");
    window.location.reload();
  }
  const request = await fetch("/upload", {
    body: files[0],
    method: "POST",
  });
  const json = await request.json();
  const id = json["id"];
  window.location.pathname = `/${id}`;
}

function onDrop(event) {
  event.preventDefault();
  handleDrop(event).then((v) => v);
}

function onDragOver(event) {
  event.preventDefault();
}

function onDragEnter(event) {
  event.preventDefault();
  droparea.classList.add("file-hovered");
}

function onDragLeave(event) {
  event.preventDefault();
  droparea.classList.remove("file-hovered");
}

droparea.addEventListener("drop", onDrop);
droparea.addEventListener("dragover", onDragOver);
droparea.addEventListener("dragenter", onDragEnter);
droparea.addEventListener("dragenter", onDragLeave);

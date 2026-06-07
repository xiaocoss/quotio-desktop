import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import MenuBarPanel from "./MenuBarPanel";

const isMenuBar =
  window.location.hash.replace(/^#/, "") === "menubar" ||
  new URLSearchParams(window.location.search).get("view") === "menubar";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isMenuBar ? <MenuBarPanel /> : <App />}
  </React.StrictMode>,
);
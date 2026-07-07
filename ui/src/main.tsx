import React from "react";
import ReactDOM from "react-dom/client";

import Settings from "./settings/Settings";
import "./settings/settings.css";

const root = document.getElementById("root");
if (root) {
  ReactDOM.createRoot(root).render(
    <React.StrictMode>
      <Settings />
    </React.StrictMode>,
  );
}

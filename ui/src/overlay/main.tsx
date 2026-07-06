import React from "react";
import ReactDOM from "react-dom/client";

import Overlay from "./Overlay";
import "./overlay.css";

const root = document.getElementById("root");
if (root !== null) {
  ReactDOM.createRoot(root).render(
    <React.StrictMode>
      <Overlay />
    </React.StrictMode>,
  );
}

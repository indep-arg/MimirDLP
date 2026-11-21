import { createApp } from "vue";
import App from "./App.vue";
import "./styles/base.css";

// The page is an app's window, not a web page: no browser menu on a right
// click, except where text can be edited or selected.
window.addEventListener("contextmenu", (event) => {
  const target = event.target as HTMLElement | null;
  if (!target?.closest("input, textarea, .selectable")) event.preventDefault();
});

createApp(App).mount("#app");

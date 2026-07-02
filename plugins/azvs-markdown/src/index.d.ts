declare module "main" {
  export function render_markdown(): I32;
}

declare module "extism:host" {
  interface user {
    asset_hub_content_read(url: PTR): PTR;
  }
}

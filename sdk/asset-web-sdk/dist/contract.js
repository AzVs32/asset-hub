//#region src/contract.generated.ts
var e = "asset-hub.plugin-api@2", t = "asset-hub.plugin-frame@2", n = "asset-hub.plugin-directory-frame@2", r = [
	"thumbnail",
	"view",
	"edit"
], i = ["thumbnail", "workspace"], a = ["executeResourceAction", "replaceResourceText"], o = [
	"executeDirectoryAction",
	"viewResource",
	"refreshDirectory",
	"navigateToDirectory",
	"editResource"
], s = [
	"text",
	"markdown",
	"html",
	"plugin_frame",
	"json",
	"media",
	"download"
], c = ["replace_content", "delete"], l = [
	"update",
	"create_child",
	"create_tree",
	"delete"
], u = r[0], d = r[1], f = r[2], p = i[0], m = i[1];
//#endregion
export { n as DIRECTORY_FRAME_CHANNEL, p as DIRECTORY_THUMBNAIL_CAPABILITY, m as DIRECTORY_WORKSPACE_CAPABILITY, e as PLUGIN_API_VERSION, f as RESOURCE_EDIT_CAPABILITY, t as RESOURCE_FRAME_CHANNEL, u as RESOURCE_THUMBNAIL_CAPABILITY, d as RESOURCE_VIEW_CAPABILITY, i as directoryActionCapabilityIds, l as directoryActionEffectKinds, o as directoryFrameMethods, s as pluginViewKinds, r as resourceActionCapabilityIds, c as resourceActionEffectKinds, a as resourceFrameMethods };

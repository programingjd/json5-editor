export function buildTargets(chromium) {
  const firefox = structuredClone(chromium);
  delete firefox.minimum_chrome_version;
  delete firefox.declarative_net_request;
  delete firefox.cross_origin_embedder_policy;
  delete firefox.cross_origin_opener_policy;
  firefox.permissions = ['webRequest', 'webRequestBlocking', 'storage', 'downloads', 'clipboardWrite'];
  firefox.background = { scripts: ['src/background.js'], type: 'module' };
  firefox.browser_specific_settings = {
    gecko: {
      id: 'json5-editor@programingjd.github.io',
      strict_min_version: '140.0',
      data_collection_permissions: { required: ['none'] },
    },
  };
  return [
    { directory: 'dist', label: 'Chromium MV3', manifest: chromium },
    { directory: 'dist-firefox', label: 'Firefox MV3', manifest: firefox },
  ];
}

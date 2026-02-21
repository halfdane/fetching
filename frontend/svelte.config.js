import adapter from '@sveltejs/adapter-static';

const config = {
  kit: {
    adapter: adapter({
      pages: 'build',
      assets: 'build',
      fallback: 'index.html', // important: SPA fallback
      strict: false           // don't error on "dynamic" routes
    })
  }
};

export default config;

// @ts-check
import {themes as prismThemes} from 'prism-react-renderer';

/** @type {import('@docusaurus/types').Config} */
const config = {
  title: 'XRPL ↔ Axelar Amplifier',
  tagline: 'Bridging XRPL with the Axelar interchain network',
  favicon: 'img/favicon.svg',

  url: 'https://xrpl.docs.axelar.network',
  baseUrl: '/',

  organizationName: 'axelarnetwork',
  projectName: 'axelar-amplifier-xrpl',

  onBrokenLinks: 'throw',
  onBrokenAnchors: 'throw',

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  presets: [
    [
      'classic',
      /** @type {import('@docusaurus/preset-classic').Options} */
      ({
        docs: {
          routeBasePath: '/',
          sidebarPath: './sidebars.js',
          editUrl:
            'https://github.com/axelarnetwork/axelar-amplifier-xrpl/edit/xrpl/website/',
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      }),
    ],
  ],

  themeConfig:
    /** @type {import('@docusaurus/preset-classic').ThemeConfig} */
    ({
      image: 'img/social-card.png',
      navbar: {
        title: 'XRPL ↔ Axelar Amplifier',
        hideOnScroll: false,
        items: [
          {
            href: 'https://github.com/axelarnetwork/axelar-amplifier-xrpl',
            label: 'GitHub',
            position: 'right',
          },
        ],
      },
      footer: {
        style: 'dark',
        links: [
          {
            title: 'Documentation',
            items: [
              {label: 'Overview', to: '/'},
              {label: 'Message Flows', to: '/message-flows'},
              {label: 'Glossary', to: '/glossary'},
            ],
          },
          {
            title: 'Source',
            items: [
              {
                label: 'axelar-amplifier-xrpl',
                href: 'https://github.com/axelarnetwork/axelar-amplifier-xrpl',
              },
              {
                label: 'axelar-amplifier (upstream)',
                href: 'https://github.com/axelarnetwork/axelar-amplifier',
              },
            ],
          },
          {
            title: 'More',
            items: [
              {
                label: 'Axelar developer docs',
                href: 'https://docs.axelar.dev',
              },
              {
                label: 'XRPL docs',
                href: 'https://xrpl.org',
              },
            ],
          },
        ],
        copyright: `Copyright © ${new Date().getFullYear()} Axelar Network. Built with Docusaurus.`,
      },
      colorMode: {
        defaultMode: 'light',
        disableSwitch: false,
        respectPrefersColorScheme: true,
      },
      prism: {
        theme: prismThemes.github,
        darkTheme: prismThemes.dracula,
        additionalLanguages: ['rust', 'solidity', 'toml', 'bash', 'json'],
      },
      tableOfContents: {
        minHeadingLevel: 2,
        maxHeadingLevel: 4,
      },
    }),

  themes: ['@docusaurus/theme-mermaid'],
  markdown: {
    mermaid: true,
    hooks: {
      onBrokenMarkdownLinks: 'warn',
    },
  },
};

export default config;

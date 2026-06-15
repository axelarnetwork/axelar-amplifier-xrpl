/** @type {import('@docusaurus/plugin-content-docs').SidebarsConfig} */
const sidebars = {
  docs: [
    // Top-level intro pages, mirroring the prefix entries in the old SUMMARY.md.
    'overview',
    'message-flows',
    'tokens-and-amounts',
    'tickets',
    'firewalling',

    {
      type: 'category',
      label: 'General Axelar Contracts',
      collapsed: false,
      items: [
        'contracts/service_registry',
        'contracts/router',
        'contracts/multisig',
        'contracts/gateway',
        'contracts/voting_verifier',
        'contracts/multisig_prover',
        'contracts/coordinator',
      ],
    },

    {
      type: 'category',
      label: 'XRPL-Specific Axelar Contracts',
      collapsed: false,
      items: [
        'contracts/xrpl_gateway',
        'contracts/xrpl_voting_verifier',
        'contracts/xrpl_multisig_prover',
      ],
    },

    // Suffix entries.
    'message_access',
    'glossary',
  ],
};

module.exports = sidebars;

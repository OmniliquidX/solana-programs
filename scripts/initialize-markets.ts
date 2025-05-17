// scripts/initialize-markets.ts 
import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { PublicKey, Keypair, Connection, SystemProgram } from "@solana/web3.js";
import { TOKEN_PROGRAM_ID, createMint, createAccount } from "@solana/spl-token";
import fs from 'fs';
import path from 'path';
import * as dotenv from 'dotenv';

// Load environment variables
dotenv.config();

// Load program IDs, assets, and wallet configurations
const programIds = JSON.parse(
  fs.readFileSync(path.join(__dirname, 'config', 'program-ids.json'), 'utf8')
);

const tokensData = JSON.parse(
  fs.readFileSync(path.join(__dirname, 'config', 'tokens.json'), 'utf8')
);

const walletsData = JSON.parse(
  fs.readFileSync(path.join(__dirname, 'config', 'wallets.json'), 'utf8')
);

const assets = JSON.parse(
  fs.readFileSync(path.join(__dirname, 'config', 'assets.json'), 'utf8')
);

// Load wallets
const deployerWallet = Keypair.fromSecretKey(
  new Uint8Array(JSON.parse(
    fs.readFileSync(
      process.env.DEPLOYER_WALLET_PATH || '~/.config/solana/id.json',
      'utf8'
    )
  ))
);

const governanceWallet = Keypair.fromSecretKey(
  new Uint8Array(walletsData.governance)
);

// Market configurations
const markets = [
  {
    name: "BTC-PERP",
    symbol: "BTC-PERP",
    assetId: "BTC",
    isPerpetual: true,
    settleWithUsdc: true,
    minBaseOrderSize: 100000, // 0.001 BTC
    tickSize: 100000, // $1.00
    takerFeeBps: 75, // 0.075%
    makerRebateBps: 25 // 0.025%
  },
  {
    name: "ETH-PERP",
    symbol: "ETH-PERP",
    assetId: "ETH",
    isPerpetual: true,
    settleWithUsdc: true,
    minBaseOrderSize: 1000000, // 0.001 ETH
    tickSize: 10000, // $0.10
    takerFeeBps: 75,
    makerRebateBps: 25
  },
  {
    name: "SOL-PERP",
    symbol: "SOL-PERP",
    assetId: "SOL",
    isPerpetual: true,
    settleWithUsdc: true,
    minBaseOrderSize: 10000000, // 0.01 SOL
    tickSize: 1000, // $0.01
    takerFeeBps: 75,
    makerRebateBps: 25
  },
  {
    name: "AAPL-PERP",
    symbol: "AAPL-PERP",
    assetId: "AAPL",
    isPerpetual: true,
    settleWithUsdc: true,
    minBaseOrderSize: 100000, // 0.01 shares
    tickSize: 1000, // $0.01
    takerFeeBps: 100, // 0.1%
    makerRebateBps: 30 // 0.03%
  },
  {
    name: "EUR/USD-PERP",
    symbol: "EUR/USD-PERP",
    assetId: "EUR/USD",
    isPerpetual: true,
    settleWithUsdc: true,
    minBaseOrderSize: 1000000, // 0.01 lot
    tickSize: 10, // $0.0001
    takerFeeBps: 80,
    makerRebateBps: 20
  },
  {
    name: "GOLD-PERP",
    symbol: "GOLD-PERP",
    assetId: "GOLD",
    isPerpetual: true,
    settleWithUsdc: true,
    minBaseOrderSize: 100000, // 0.001 troy oz
    tickSize: 10000, // $0.10
    takerFeeBps: 80,
    makerRebateBps: 20
  }
];

async function initializeMarkets() {
  // Configure connection
  const connection = new Connection(
    programIds.cluster === 'devnet'
      ? process.env.DEVNET_RPC_URL || 'https://api.devnet.solana.com'
      : 'http://localhost:8899',
    'confirmed'
  );
  
  // Configure provider with deployer wallet
  const provider = new anchor.AnchorProvider(
    connection,
    new anchor.Wallet(deployerWallet),
    { commitment: 'confirmed' }
  );
  
  // Get Registry, CLOB, and Price Router programs
  const registryIdl = JSON.parse(
    fs.readFileSync(`target/idl/omniliquid_registry.json`, 'utf8')
  );
  
  const clobIdl = JSON.parse(
    fs.readFileSync(`target/idl/omniliquid_clob.json`, 'utf8')
  );
  
  const registryProgram = new Program(
    registryIdl,
    new PublicKey(programIds.registry),
    provider
  );
  
  const clobProgram = new Program(
    clobIdl,
    new PublicKey(programIds.clob),
    provider
  );
  
  // Find Registry PDA
  const [registryAccount] = PublicKey.findProgramAddressSync(
    [Buffer.from("registry")],
    registryProgram.programId
  );
  
  console.log(`Using Registry at: ${registryAccount.toString()}`);
  console.log(`Using USDC mint: ${tokensData.usdc}`);
  
  // Initialize each market
  const initializedMarkets = [];
  console.log(`\nInitializing ${markets.length} markets...`);
  
  for (const market of markets) {
    try {
      console.log(`\nInitializing ${market.name}...`);
      
      // For each market, we need to create a synthetic base mint
      const baseMint = await createMint(
        connection,
        deployerWallet,
        deployerWallet.publicKey,
        null,
        9 // 9 decimals like most Solana tokens
      );
      
      console.log(`Created base mint for ${market.name}: ${baseMint.toString()}`);
      
      // Create market account
      const marketKeypair = Keypair.generate();
      
      // Create orderbook account
      const orderbookKeypair = Keypair.generate();
      
      // Find vault signer PDA
      const [vaultSigner, vaultSignerBump] = PublicKey.findProgramAddressSync(
        [Buffer.from("vault_signer"), marketKeypair.publicKey.toBuffer()],
        clobProgram.programId
      );
      
      // Create base and quote vaults
      const baseVault = await createAccount(
        connection,
        deployerWallet,
        baseMint,
        vaultSigner
      );
      
      const quoteVault = await createAccount(
        connection,
        deployerWallet,
        new PublicKey(tokensData.usdc),
        vaultSigner
      );
      
      console.log(`Created vaults for ${market.name}:`);
      console.log(`  Base vault: ${baseVault.toString()}`);
      console.log(`  Quote vault: ${quoteVault.toString()}`);
      
      // Initialize the market
      await clobProgram.methods
        .initialize(
          new anchor.BN(market.minBaseOrderSize),
          new anchor.BN(market.tickSize),
          market.takerFeeBps,
          market.makerRebateBps,
          market.name,
          market.symbol,
          market.assetId,
          market.isPerpetual,
          market.settleWithUsdc
        )
        .accounts({
          market: marketKeypair.publicKey,
          orderbook: orderbookKeypair.publicKey,
          baseMint,
          quoteMint: new PublicKey(tokensData.usdc),
          baseVault,
          quoteVault,
          vaultSigner,
          authority: deployerWallet.publicKey,
          registry: registryAccount,
          systemProgram: SystemProgram.programId,
          tokenProgram: TOKEN_PROGRAM_ID,
          rent: anchor.web3.SYSVAR_RENT_PUBKEY,
        })
        .signers([marketKeypair, orderbookKeypair])
        .rpc();
      
      console.log(`✅ ${market.name} initialized successfully`);
      console.log(`  Market: ${marketKeypair.publicKey.toString()}`);
      console.log(`  Orderbook: ${orderbookKeypair.publicKey.toString()}`);
      
      initializedMarkets.push({
        ...market,
        marketId: marketKeypair.publicKey.toString(),
        orderbookId: orderbookKeypair.publicKey.toString(),
        baseMint: baseMint.toString(),
        baseVault: baseVault.toString(),
        quoteVault: quoteVault.toString(),
      });
    } catch (e) {
      console.error(`❌ Error initializing ${market.name}:`, e);
    }
  }
  
  console.log("\nMarket initialization completed!");
  
  // Save market data to a file
  fs.writeFileSync(
    path.join(__dirname, 'config', 'markets.json'),
    JSON.stringify(initializedMarkets, null, 2)
  );
}

// Run market initialization
initializeMarkets().catch(err => {
  console.error("Fatal error:", err);
  process.exit(1);
});
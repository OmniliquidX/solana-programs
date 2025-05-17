// scripts/register-assets.ts
import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { PublicKey, Keypair, Connection } from "@solana/web3.js";
import fs from 'fs';
import path from 'path';
import * as dotenv from 'dotenv';

// Load environment variables
dotenv.config();

// Load program IDs and wallet configurations
const programIds = JSON.parse(
  fs.readFileSync(path.join(__dirname, 'config', 'program-ids.json'), 'utf8')
);

const walletsData = JSON.parse(
  fs.readFileSync(path.join(__dirname, 'config', 'wallets.json'), 'utf8')
);

// Load governance wallet for admin operations
const governanceWallet = Keypair.fromSecretKey(
  new Uint8Array(walletsData.governance)
);

// Pyth price feed mapping (devnet)
const PYTH_PRICE_FEEDS = {
  "BTC/USD": new PublicKey("HovQMDrbAgAYPCmHVSrezcSmkMtXSSUsLDFANExrZh2J"),
  "ETH/USD": new PublicKey("EdVCmQ9FSPcVe5YySXDPCRmc8aDQLKJ9xvYBMZPie1Vw"),
  "SOL/USD": new PublicKey("J83w4HKfqxwcq3BEMMkPFSppX3gqekLyLJBexebFVkix"),
  "AAPL": new PublicKey("5yixRcKtcs5BZ1K2FsLFwmES1MyA92d6efvijjVevQkw"),
  "TSLA": new PublicKey("3Mnn2fX6rQyUsyELYms1sBJyChWofzSNRoqYzvgMVz5E"),
  "MSFT": new PublicKey("8s9NADL7iQnMpZTsyM64qwZB7wCshB4WdE1s7ZswXhPJ"),
  "EUR/USD": new PublicKey("8qFUgxVE2sLhK4cAVtiZd3kiAqkVwA8bA94VWQvsAbVf"),
  "GBP/USD": new PublicKey("B1oNGyQcTG3jchtxdGJvSKnvrYCm8cmXTDQJPBsXqxeJ"),
  "JPY/USD": new PublicKey("BLM5vgxJnsJhJSCenj5LF4XhNpjrRHxqQ8jLYYAFmX8s"),
  "XAU/USD": new PublicKey("9YsFRbGHvxjRLGhV1LJBsJQUjUekitPv3ynJKWzHS8Ao"), // Gold
  "XAG/USD": new PublicKey("B2bW27xZyqMwNGivnzF9zHtLgJfaKjBTA1C3LJKzG5ui"), // Silver
  "BRENT/USD": new PublicKey("4amtaGQJzEXPtWmZh7vBGwfM8YJXxRnCbLbSLLYYZrFe"), // Oil
};

// Asset configurations
const assets = [
  // Crypto
  {
    assetId: "BTC",
    assetType: 0, // Crypto
    pythPriceFeed: PYTH_PRICE_FEEDS["BTC/USD"],
    minOrderSize: 100000, // 0.001 BTC in satoshis
    maxLeverage: 100, // 100x max leverage
    maintenanceMarginRatio: 500, // 5%
    liquidationFee: 250, // 2.5%
    fundingRateMultiplier: 100, // 1x standard funding rate
    active: true
  },
  {
    assetId: "ETH",
    assetType: 0, // Crypto
    pythPriceFeed: PYTH_PRICE_FEEDS["ETH/USD"],
    minOrderSize: 1000000, // 0.001 ETH in wei
    maxLeverage: 100,
    maintenanceMarginRatio: 500,
    liquidationFee: 250,
    fundingRateMultiplier: 100,
    active: true
  },
  {
    assetId: "SOL",
    assetType: 0, // Crypto
    pythPriceFeed: PYTH_PRICE_FEEDS["SOL/USD"],
    minOrderSize: 10000000, // 0.01 SOL in lamports
    maxLeverage: 100,
    maintenanceMarginRatio: 500,
    liquidationFee: 250,
    fundingRateMultiplier: 100,
    active: true
  },
  
  // Stocks
  {
    assetId: "AAPL",
    assetType: 1, // Stock
    pythPriceFeed: PYTH_PRICE_FEEDS["AAPL"],
    minOrderSize: 100000, // 0.01 shares
    maxLeverage: 10, // 10x max for stocks
    maintenanceMarginRatio: 1000, // 10%
    liquidationFee: 150, // 1.5%
    fundingRateMultiplier: 120, // 1.2x funding rate
    active: true
  },
  {
    assetId: "TSLA",
    assetType: 1, // Stock
    pythPriceFeed: PYTH_PRICE_FEEDS["TSLA"],
    minOrderSize: 100000,
    maxLeverage: 10,
    maintenanceMarginRatio: 1000,
    liquidationFee: 150,
    fundingRateMultiplier: 120,
    active: true
  },
  {
    assetId: "MSFT",
    assetType: 1, // Stock
    pythPriceFeed: PYTH_PRICE_FEEDS["MSFT"],
    minOrderSize: 100000,
    maxLeverage: 10,
    maintenanceMarginRatio: 1000,
    liquidationFee: 150,
    fundingRateMultiplier: 120,
    active: true
  },
  
  // Forex
  {
    assetId: "EUR/USD",
    assetType: 2, // Forex
    pythPriceFeed: PYTH_PRICE_FEEDS["EUR/USD"],
    minOrderSize: 1000000, // 0.01 lot
    maxLeverage: 30, // 30x for forex
    maintenanceMarginRatio: 750, // 7.5%
    liquidationFee: 150,
    fundingRateMultiplier: 80, // 0.8x funding rate
    active: true
  },
  {
    assetId: "GBP/USD",
    assetType: 2, // Forex
    pythPriceFeed: PYTH_PRICE_FEEDS["GBP/USD"],
    minOrderSize: 1000000,
    maxLeverage: 30,
    maintenanceMarginRatio: 750,
    liquidationFee: 150,
    fundingRateMultiplier: 80,
    active: true
  },
  {
    assetId: "JPY/USD",
    assetType: 2, // Forex
    pythPriceFeed: PYTH_PRICE_FEEDS["JPY/USD"],
    minOrderSize: 1000000,
    maxLeverage: 30,
    maintenanceMarginRatio: 750,
    liquidationFee: 150,
    fundingRateMultiplier: 80,
    active: true
  },
  
  // Commodities
  {
    assetId: "GOLD",
    assetType: 3, // Commodity
    pythPriceFeed: PYTH_PRICE_FEEDS["XAU/USD"],
    minOrderSize: 100000, // 0.001 troy ounce
    maxLeverage: 20, // 20x for commodities
    maintenanceMarginRatio: 800, // 8%
    liquidationFee: 200, // 2%
    fundingRateMultiplier: 110, // 1.1x funding rate
    active: true
  },
  {
    assetId: "SILVER",
    assetType: 3, // Commodity
    pythPriceFeed: PYTH_PRICE_FEEDS["XAG/USD"],
    minOrderSize: 100000,
    maxLeverage: 20,
    maintenanceMarginRatio: 800,
    liquidationFee: 200,
    fundingRateMultiplier: 110,
    active: true
  },
  {
    assetId: "OIL",
    assetType: 3, // Commodity
    pythPriceFeed: PYTH_PRICE_FEEDS["BRENT/USD"],
    minOrderSize: 100000, // 0.01 barrel
    maxLeverage: 20,
    maintenanceMarginRatio: 800,
    liquidationFee: 200,
    fundingRateMultiplier: 110,
    active: true
  }
];

async function registerAssets() {
  // Configure connection
  const connection = new Connection(
    programIds.cluster === 'devnet'
      ? process.env.DEVNET_RPC_URL || 'https://api.devnet.solana.com'
      : 'http://localhost:8899',
    'confirmed'
  );
  
  // Configure provider with governance wallet
  const provider = new anchor.AnchorProvider(
    connection,
    new anchor.Wallet(governanceWallet),
    { commitment: 'confirmed' }
  );
  
  // Get Registry program
  const registryIdl = JSON.parse(
    fs.readFileSync(`target/idl/omniliquid_registry.json`, 'utf8')
  );
  
  const registryProgram = new Program(
    registryIdl,
    new PublicKey(programIds.registry),
    provider
  );
  
  // Find Registry PDA
  const [registryAccount] = PublicKey.findProgramAddressSync(
    [Buffer.from("registry")],
    registryProgram.programId
  );
  
  console.log(`Using Registry at: ${registryAccount.toString()}`);
  console.log(`Governance Wallet: ${governanceWallet.publicKey.toString()}`);
  
  // Register each asset
  console.log(`\nRegistering ${assets.length} assets...`);
  
  for (const asset of assets) {
    try {
      console.log(`Registering ${asset.assetId}...`);
      
      await registryProgram.methods
        .registerAsset(
          asset.assetId,
          asset.assetType,
          asset.pythPriceFeed,
          new anchor.BN(asset.minOrderSize),
          asset.maxLeverage,
          asset.maintenanceMarginRatio,
          asset.liquidationFee,
          asset.fundingRateMultiplier,
          asset.active
        )
        .accounts({
          registry: registryAccount,
          gov: governanceWallet.publicKey,
        })
        .rpc();
      
      console.log(`✅ ${asset.assetId} registered successfully`);
    } catch (e) {
      console.error(`❌ Error registering ${asset.assetId}:`, e);
      
      // Try updating if it already exists
      try {
        console.log(`Trying to update ${asset.assetId} instead...`);
        
        await registryProgram.methods
          .updateAsset(
            asset.assetId,
            new anchor.BN(asset.minOrderSize),
            asset.maxLeverage,
            asset.maintenanceMarginRatio,
            asset.liquidationFee,
            asset.fundingRateMultiplier,
            asset.active
          )
          .accounts({
            registry: registryAccount,
            gov: governanceWallet.publicKey,
          })
          .rpc();
        
        console.log(`✅ ${asset.assetId} updated successfully`);
      } catch (updateErr) {
        console.error(`❌ Error updating ${asset.assetId}:`, updateErr);
      }
    }
  }
  
  console.log("\nAsset registration completed!");
  
  // Save asset data to a file
  fs.writeFileSync(
    path.join(__dirname, 'config', 'assets.json'),
    JSON.stringify(assets, null, 2)
  );
}

// Run asset registration
registerAssets().catch(err => {
  console.error("Fatal error:", err);
  process.exit(1);
});
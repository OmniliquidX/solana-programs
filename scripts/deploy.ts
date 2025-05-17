 // scripts/deploy.ts
import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { PublicKey, Keypair, Connection, SystemProgram, LAMPORTS_PER_SOL } from "@solana/web3.js";
import { TOKEN_PROGRAM_ID, createMint, createAccount, mintTo } from "@solana/spl-token";
import fs from 'fs';
import path from 'path';
import * as dotenv from 'dotenv';

// Load environment variables
dotenv.config();

// Command line args
const args = process.argv.slice(2);
const isDevnet = args.includes('--cluster=devnet') || args.includes('--cluster=devnet');
const cluster = 'devnet'

console.log(`Deploying to ${cluster}...`);

// Helper function to load program keypairs
function loadKeypair(name: string): Keypair {
  try {
    const keypairPath = `target/deploy/${name}-keypair.json`;
    if (fs.existsSync(keypairPath)) {
      const content = fs.readFileSync(keypairPath, 'utf-8');
      const keypairData = Uint8Array.from(JSON.parse(content));
      return Keypair.fromSecretKey(keypairData);
    }
  } catch (e) {
    console.error(`Error loading keypair for ${name}:`, e);
  }
  
  // If keypair doesn't exist, generate a new one
  const keypair = Keypair.generate();
  const keypairPath = `target/deploy/${name}-keypair.json`;
  
  // Make sure the directory exists
  if (!fs.existsSync('target/deploy')) {
    fs.mkdirSync('target/deploy', { recursive: true });
  }
  
  fs.writeFileSync(
    keypairPath,
    JSON.stringify(Array.from(keypair.secretKey))
  );
  
  return keypair;
}

// Load the program IDLs
async function loadIdl(name: string, programId: PublicKey, provider: anchor.AnchorProvider) {
  try {
    const idlPath = `target/idl/${name}.json`;
    if (fs.existsSync(idlPath)) {
      const idlJson = JSON.parse(fs.readFileSync(idlPath, 'utf-8'));
      return idlJson;
    } else {
      // Try to fetch from the chain
      return await anchor.Program.fetchIdl(programId, provider);
    }
  } catch (e) {
    console.error(`Error loading IDL for ${name}:`, e);
    throw e;
  }
}

// Deploy all programs
async function deployPrograms() {
  // Configure the client
  const connection = new Connection(
    'https://api.devnet.solana.com',
    'confirmed'
  );
  
  const wallet = new anchor.Wallet(loadKeypair('id'));
  const provider = new anchor.AnchorProvider(connection, wallet, {
    commitment: 'confirmed',
  });
  anchor.setProvider(provider);
  
  console.log(`Using wallet: ${wallet.publicKey.toString()}`);
  
  // Check wallet balance
  const balance = await connection.getBalance(wallet.publicKey);
  console.log(`Wallet balance: ${balance / LAMPORTS_PER_SOL} SOL`);
  
  if (balance < 2 * LAMPORTS_PER_SOL) {
    if (isDevnet) {
      // Try to airdrop on devnet
      console.log("Requesting airdrop...");
      try {
        const sig = await connection.requestAirdrop(wallet.publicKey, 2 * LAMPORTS_PER_SOL);
        await connection.confirmTransaction(sig);
        console.log(`New balance: ${await connection.getBalance(wallet.publicKey) / LAMPORTS_PER_SOL} SOL`);
      } catch (e) {
        console.warn("Airdrop failed. Make sure your wallet has enough SOL.");
      }
    } else {
      console.warn("Warning: Low balance. Make sure your wallet has enough SOL.");
    }
  }
  
  // Load program keypairs
  const registryKeypair = loadKeypair('omniliquid_registry');
  const clobKeypair = loadKeypair('omniliquid_clob');
  const priceRouterKeypair = loadKeypair('omniliquid_price_router');
  const tradingStorageKeypair = loadKeypair('omniliquid_trading_storage');
  const omniTokenKeypair = loadKeypair('omniliquid_omni_token');
  const olpVaultKeypair = loadKeypair('omniliquid_olp_vault');
  
  console.log("Program IDs:");
  console.log("Registry:", registryKeypair.publicKey.toString());
  console.log("CLOB:", clobKeypair.publicKey.toString());
  console.log("Price Router:", priceRouterKeypair.publicKey.toString());
  console.log("Trading Storage:", tradingStorageKeypair.publicKey.toString());
  console.log("OMNI Token:", omniTokenKeypair.publicKey.toString());
  console.log("OLP Vault:", olpVaultKeypair.publicKey.toString());
  
  // Save program IDs to a JSON file for easier reference
  const programIdsPath = path.join(__dirname, 'config', 'program-ids.json');
  fs.writeFileSync(
    programIdsPath,
    JSON.stringify({
      registry: registryKeypair.publicKey.toString(),
      clob: clobKeypair.publicKey.toString(),
      priceRouter: priceRouterKeypair.publicKey.toString(),
      tradingStorage: tradingStorageKeypair.publicKey.toString(),
      omniToken: omniTokenKeypair.publicKey.toString(),
      olpVault: olpVaultKeypair.publicKey.toString(),
      cluster
    }, null, 2)
  );
  
  // Build and deploy all programs
  console.log("\nBuilding and deploying programs...");
  
  // You'd typically run `anchor deploy` here, but for simplicity we'll assume the programs are already built

  // Initialize the Registry Program
  console.log("\n1. Initializing Registry Program...");
  const registryIdl = await loadIdl('omniliquid_registry', registryKeypair.publicKey, provider);
  const registryProgram = new Program(registryIdl, registryKeypair.publicKey, provider);
  
  const [registryAccount, registryBump] = PublicKey.findProgramAddressSync(
    [Buffer.from("registry")],
    registryProgram.programId
  );
  
  const [registryAuthority, _] = PublicKey.findProgramAddressSync(
    [Buffer.from("authority"), registryAccount.toBuffer()],
    registryProgram.programId
  );
  
  // Create governance, dev, and manager wallets
  const governanceWallet = Keypair.generate();
  const devWallet = Keypair.generate();
  const managerWallet = Keypair.generate();
  
  console.log("Governance Wallet:", governanceWallet.publicKey.toString());
  console.log("Dev Wallet:", devWallet.publicKey.toString());
  console.log("Manager Wallet:", managerWallet.publicKey.toString());
  
  // Save these wallets for future use
  fs.writeFileSync(
    path.join(__dirname, 'config', 'wallets.json'),
    JSON.stringify({
      governance: Array.from(governanceWallet.secretKey),
      dev: Array.from(devWallet.secretKey),
      manager: Array.from(managerWallet.secretKey),
    }, null, 2)
  );
  
  // Initialize the registry
  try {
    await registryProgram.methods
      .initialize(
        governanceWallet.publicKey,
        devWallet.publicKey,
        managerWallet.publicKey
      )
      .accounts({
        registry: registryAccount,
        owner: wallet.publicKey,
        registryAuthority,
        systemProgram: SystemProgram.programId,
      })
      .rpc();
    
    console.log("Registry initialized:", registryAccount.toString());
  } catch (e) {
    console.error("Error initializing registry:", e);
    throw e;
  }
  
  // Create USDC mock token (for devnet testing)
  console.log("\n2. Creating mock USDC token...");
  const usdcMint = await createMint(
    connection,
    wallet.payer,
    wallet.publicKey,
    null,
    6
  );
  console.log("USDC Mint:", usdcMint.toString());
  
  // Create deployer's USDC account
  const usdcAccount = await createAccount(
    connection,
    wallet.payer,
    usdcMint,
    wallet.publicKey
  );
  console.log("Deployer USDC Account:", usdcAccount.toString());
  
  // Mint some USDC to the deployer
  await mintTo(
    connection,
    wallet.payer,
    usdcMint,
    usdcAccount,
    wallet.payer,
    1_000_000_000_000 // 1 million USDC
  );
  
  // Initialize OMNI Token
  console.log("\n3. Initializing OMNI Token...");
  const omniIdl = await loadIdl('omniliquid_omni_token', omniTokenKeypair.publicKey, provider);
  const omniProgram = new Program(omniIdl, omniTokenKeypair.publicKey, provider);
  
  const [tokenAuthority, tokenAuthorityBump] = PublicKey.findProgramAddressSync(
    [Buffer.from("token_authority")],
    omniProgram.programId
  );
  
  const omniMint = Keypair.generate();
  
  try {
    const [tokenConfig] = PublicKey.findProgramAddressSync(
      [Buffer.from("token_config"), omniMint.publicKey.toBuffer()],
      omniProgram.programId
    );
    
    await omniProgram.methods
      .initialize(
        "Omniliquid",
        "OMNI",
        "https://omniliquid.xyz/token-metadata.json",
        1_000_000_000 * 10**9 // 1 billion tokens with 9 decimals
      )
      .accounts({
        tokenConfig,
        mint: omniMint.publicKey,
        tokenAuthority,
        authority: wallet.publicKey,
        payer: wallet.publicKey,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .signers([omniMint])
      .rpc();
    
    console.log("OMNI Token initialized:", omniMint.publicKey.toString());
    
    // Save OMNI mint address
    fs.writeFileSync(
      path.join(__dirname, 'config', 'tokens.json'),
      JSON.stringify({
        usdc: usdcMint.toString(),
        omni: omniMint.publicKey.toString(),
      }, null, 2)
    );
  } catch (e) {
    console.error("Error initializing OMNI token:", e);
    throw e;
  }

  // Initialize other programs and set up markets
  console.log("\nDeployment completed successfully!");
  console.log("For the next steps, run:");
  console.log("1. yarn register-assets - to register RWA assets");
  console.log("2. yarn initialize-markets - to initialize markets");
}

// Run the deployment
deployPrograms().catch(err => {
  console.error(err);
  process.exit(1);
});
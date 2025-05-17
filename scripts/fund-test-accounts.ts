// scripts/fund-test-accounts.ts
import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { PublicKey, Keypair, Connection } from "@solana/web3.js";
import { TOKEN_PROGRAM_ID, createAccount, mintTo } from "@solana/spl-token";
import fs from 'fs';
import path from 'path';
import * as dotenv from 'dotenv';

// Load environment variables
dotenv.config();

// Load program IDs and tokens
const programIds = JSON.parse(
  fs.readFileSync(path.join(__dirname, 'config', 'program-ids.json'), 'utf8')
);

const tokensData = JSON.parse(
  fs.readFileSync(path.join(__dirname, 'config', 'tokens.json'), 'utf8')
);

const markets = JSON.parse(
  fs.readFileSync(path.join(__dirname, 'config', 'markets.json'), 'utf8')
);

// Load deployer wallet
const deployerWallet = Keypair.fromSecretKey(
  new Uint8Array(JSON.parse(
    fs.readFileSync(
      process.env.DEPLOYER_WALLET_PATH || '~/.config/solana/id.json', 
      'utf8'
    )
  ))
);

// Test accounts to fund
const testAccounts = Array(5).fill(0).map(() => Keypair.generate());

async function fundTestAccounts() {
  // Configure connection
  const connection = new Connection(
    programIds.cluster === 'devnet'
      ? process.env.DEVNET_RPC_URL || 'https://api.devnet.solana.com'
      : 'http://localhost:8899',
    'confirmed'
  );
  
  console.log(`Funding ${testAccounts.length} test accounts on ${programIds.cluster}...`);
  
  // Fund with SOL
  for (let i = 0; i < testAccounts.length; i++) {
    const account = testAccounts[i];
    console.log(`\nFunding account ${i+1}: ${account.publicKey.toString()}`);
    
    try {
      // Airdrop or transfer SOL
      if (programIds.cluster === 'devnet') {
        // Airdrop on devnet (may fail due to limits)
        try {
          const airdropSig = await connection.requestAirdrop(
            account.publicKey,
            1 * anchor.web3.LAMPORTS_PER_SOL
          );
          await connection.confirmTransaction(airdropSig);
          console.log(`  ✅ Airdropped 1 SOL`);
        } catch (e) {
          console.warn(`  ⚠️ Airdrop failed, transferring from deployer instead`);
          
          // Transfer from deployer if airdrop fails
          const transferSig = await connection.sendTransaction(
            new anchor.web3.Transaction().add(
              anchor.web3.SystemProgram.transfer({
                fromPubkey: deployerWallet.publicKey,
                toPubkey: account.publicKey,
                lamports: 0.1 * anchor.web3.LAMPORTS_PER_SOL
              })
            ),
            [deployerWallet]
          );
          await connection.confirmTransaction(transferSig);
          console.log(`  ✅ Transferred 0.1 SOL from deployer`);
        }
      } else {
        // Local - transfer from deployer
        const transferSig = await connection.sendTransaction(
          new anchor.web3.Transaction().add(
            anchor.web3.SystemProgram.transfer({
              fromPubkey: deployerWallet.publicKey,
              toPubkey: account.publicKey,
              lamports: 2 * anchor.web3.LAMPORTS_PER_SOL
            })
          ),
          [deployerWallet]
        );
        await connection.confirmTransaction(transferSig);
        console.log(`  ✅ Transferred 2 SOL from deployer`);
      }
      
      // Create USDC account and fund it
      const usdcAccount = await createAccount(
        connection,
        deployerWallet,
        new PublicKey(tokensData.usdc),
        account.publicKey
      );
      
      await mintTo(
        connection,
        deployerWallet,
        new PublicKey(tokensData.usdc),
        usdcAccount,
        deployerWallet,
        10_000_000_000 // 10,000 USDC
      );
      
      console.log(`  ✅ Created USDC account: ${usdcAccount.toString()}`);
      console.log(`  ✅ Funded with 10,000 USDC`);
      
      // For each market, create a base token account
      for (const market of markets) {
        const baseAccount = await createAccount(
          connection,
          deployerWallet,
          new PublicKey(market.baseMint),
          account.publicKey
        );
        
        await mintTo(
          connection,
          deployerWallet,
          new PublicKey(market.baseMint),
          baseAccount,
          deployerWallet,
          1_000_000_000_000 // 1,000 base tokens
        );
        
        console.log(`  ✅ Created ${market.symbol} base account: ${baseAccount.toString()}`);
        console.log(`  ✅ Funded with 1,000 ${market.symbol.replace('-PERP', '')}`);
      }
      
      // Save account info to file
      fs.writeFileSync(
        path.join(__dirname, 'config', `test-account-${i+1}.json`),
        JSON.stringify({
          publicKey: account.publicKey.toString(),
          secretKey: Array.from(account.secretKey),
        }, null, 2)
      );
      
      console.log(`  📝 Saved account ${i+1} to config/test-account-${i+1}.json`);
    } catch (e) {
      console.error(`  ❌ Error funding account ${i+1}:`, e);
    }
  }
  
  console.log("\nTest account funding completed!");
}

// Run funding script
fundTestAccounts().catch(err => {
  console.error("Fatal error:", err);
  process.exit(1);
});
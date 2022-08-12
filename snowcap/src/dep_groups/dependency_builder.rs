/*
Functionalities
    1. Log dependency between nodes
    2. Given the combination of configuration orderings and on which configuration
    does certain next hop changes, reassemble a dependency object (DAG)  
    3. When synthesis has finished, return an executor object; given a network object,
    the executor object can apply changes to the network
*/
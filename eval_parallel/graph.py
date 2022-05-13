from turtle import color
import matplotlib.pyplot as plt
import os
import numpy as np

def read_file_get_data(path):
    file = open(path)
    iter_column = []
    time_column = []
    while file.readline() != "":
        content = file.readline().split()
        iter_column.append(int(content[0]))
        time_column.append(int(content[1])/1000)
    return (iter_column, time_column)

lst1 = []
lst2 = []
iter1 = []
for i in range(1, 51):
    path_parallel = "./parallel_evaluation_" + str(i) + ".txt"
    path_trta = "./trta_evaluation_" + str(i) + ".txt"
    iter1, time1 = read_file_get_data(path_parallel)
    iter2, time2 = read_file_get_data(path_trta)
    lst1.append(time1)
    lst2.append(time2)

parallel_data_all_tests = [[] for i in range(len(iter1))]
trta_data_all_tests = [[] for i in range(len(iter1))]
for i in range(len(iter1)):
    for j in range(50):
        parallel_data_all_tests[i].append(lst1[j][i])
        trta_data_all_tests[i].append(lst2[j][i])

parallel_data_final = []
trta_data_final = []

for i in range(len(parallel_data_all_tests)):
    parallel_data_all_tests[i].sort()
    trta_data_all_tests[i].sort()
    median_parallel = parallel_data_all_tests[i][len(parallel_data_all_tests[i]) // 2]
    median_trta = trta_data_all_tests[i][len(trta_data_all_tests[i]) // 2]
    parallel_data_final.append(median_parallel)
    trta_data_final.append(median_trta)

# print(parallel_data_final)
# print(trta_data_final)

x1 = np.array(iter1)
y1 = np.array(trta_data_final)
y2 = np.array(parallel_data_final)

# plt.axis('equal')
plt.xlabel("Number of commands")
plt.ylabel("Seconds")
plt.plot(x1, y1, color='red')
plt.plot(x1, y2, color='blue')
plt.text(80, 20, "Snowcap")
plt.text(80, 1, "ParaNet")
plt.xticks(np.arange(0, x1[-1], 25))
plt.yticks(np.arange(0, y1[-1], 2))

plt.savefig('./plot.png')